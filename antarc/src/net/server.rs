use std::{
    io,
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
    time::{Duration, Instant},
};

use log::{debug, error, warn};

use super::SendTo;
use crate::{
    events::Event,
    host::{AwaitingConnectionAck, Connected, Disconnected, Host, RequestingConnection},
    net::{
        client::{receiver, sender},
        NetManager, NetworkResource,
    },
    packet::{
        header::Header, ConnectionId, Encoded, Footer, Packet, PacketKind, Payload, Queued, Sent,
        Sequence, CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST, DATA_TRANSFER,
        HEARTBEAT,
    },
};

pub type PacketId = u64;

const CHECK_ANTARC_QUEUE: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub struct Server {
    id_tracker: PacketId,
    received_tracker: usize,
    last_antarc_queue_check: Duration,
    connection_id_tracker: ConnectionId,
    disconnected: Vec<Host<Disconnected>>,
    requesting_connection: Vec<Host<RequestingConnection>>,
    awaiting_connection_ack: Vec<Host<AwaitingConnectionAck>>,
    connected: Vec<Host<Connected>>,
}

impl NetManager<Server> {
    pub fn new_server(address: &SocketAddr) -> Self {
        let server = Server {
            id_tracker: 0,
            last_antarc_queue_check: Duration::default(),
            connection_id_tracker: unsafe { ConnectionId::new_unchecked(1) },
            disconnected: Vec::with_capacity(8),
            requesting_connection: Vec::with_capacity(8),
            awaiting_connection_ack: Vec::with_capacity(8),
            connected: Vec::with_capacity(8),
            received_tracker: 0,
        };
        let net_manager = NetManager::new(address, server);
        net_manager
    }

    pub fn enqueue(&mut self, message: Vec<u8>) -> PacketId {
        let id = self.kind.id_tracker;

        let state = Queued {
            time: self.timer.elapsed(),
        };
        let packet = Packet {
            id,
            state,
            kind: PacketKind::DataTransfer,
        };

        for host in self.kind.connected {
            self.user_queue.push(packet);
        }

        self.payload_queue.insert(id, Payload(message));
        self.kind.id_tracker += 1;

        id
    }

    pub fn cancel_packet(&mut self, packet_id: PacketId) -> bool {
        self.user_queue
            .drain_filter(|(_, queued, _)| queued.id == packet_id)
            .next()
            .is_some()
    }

    // TODO(alex) 2021-05-17: It's probably a good idea to start working on this before going
    // further, to validate that the ideas I had so far are working. Make this the focus.
    pub fn tick(&mut self) -> Result<usize, String> {
        let mut new_events = Vec::with_capacity(8);

        self.network
            .poll
            .poll(&mut self.network.events, Some(Duration::from_millis(150)))
            .unwrap();

        for event in self.network.events.iter() {
            if event.is_readable() {
                self.events.push(Event::ReadyToReceive);
            }

            if event.is_writable() {
                self.events.push(Event::ReadyToSend);
            }
        }

        for event in self.events.drain(..) {
            match event {
                Event::ReadyToReceive => {
                    debug!("Handling ReadyToReceive");

                    loop {
                        match receiver(
                            self.kind.id_tracker,
                            &mut self.buffer,
                            &self.network.udp_socket,
                            &self.timer,
                        ) {
                            Ok(received) => {
                                debug!("Received new packet {:#?}.", received);
                                new_events.push(Event::ReceivedPacket { received });
                                self.kind.id_tracker += 1;
                                self.kind.received_tracker += 1;
                            }
                            Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                                warn!("Would block on recv_from {:?}", fail);
                                break;
                            }
                            Err(fail) => {
                                warn!("Failed recv_from with {:?}", fail);
                                break;
                            }
                        }
                    }
                }
                Event::ReadyToSend => {
                    debug!("Handling ReadyToSend");
                    // TODO(alex) 2021-05-18: What to do here ends up being a big problem, and I
                    // can't think of a good solution.
                    //
                    // The packets have an address, so if we were to just send it here, everything
                    // would be fine, but how do we get the sequence? We need to find a matching
                    // address in one of the host vectors, which depend on the host state, and get
                    // the sequence from there. This reqires checking both the `Sent` and `Acked`
                    // lists of every kind of host.
                    //
                    // A similar thing would have to be done on the `Received` and `Retrieved` to
                    // get the correct ack value, not even talking about `past_acks`.
                    //
                    // ADD(alex) 2021-05-18: The server won't send the same packet to every client,
                    // for example, when client B requests a connection, then the server will send
                    // a connection accepted to B, but will send a data transfer to client A, which
                    // is already connected.
                    'sender: loop {
                        if self.timer.elapsed()
                            > CHECK_ANTARC_QUEUE + self.kind.last_antarc_queue_check
                        {
                            if let Some((address, queued)) = self.antarc_queue.first() {
                                match queued.kind {
                                    PacketKind::ConnectionRequest => {
                                        error!("Server cannot sent connection request.");
                                        unreachable!();
                                    }
                                    PacketKind::ConnectionAccepted => {
                                        debug!("Sending connection accepted");
                                        if let Some(host) = self
                                            .kind
                                            .requesting_connection
                                            .iter()
                                            .find(|host| host.address == *address)
                                        {
                                            debug!("to host {:#?}", host);

                                            let header = Header::connection_accepted();
                                            let payload = Payload::default();
                                            debug_assert_eq!(header.payload_length, 0);

                                            let connection_id = self.kind.connection_id_tracker;
                                            let destination = host.address;

                                            let (bytes, footer) = match Packet::encode(
                                                &payload,
                                                &header,
                                                Some(connection_id),
                                            ) {
                                                Ok(encoded) => encoded,
                                                Err(fail) => {
                                                    error!("Failed encoding packet {:#?}.", fail);
                                                    new_events.push(Event::FailedEncodingPacket {
                                                        queued: queued.clone(),
                                                    });
                                                    break 'sender;
                                                }
                                            };
                                            match self
                                                .network
                                                .udp_socket
                                                .send_to(&bytes, destination)
                                            {
                                                Ok(num_sent) => {
                                                    debug!(
                                                        "Server sent packet {:#?} to {:#?}.",
                                                        queued, destination
                                                    );
                                                    debug_assert!(num_sent > 0);

                                                    let packet = queued.sent(
                                                        header,
                                                        footer,
                                                        destination,
                                                        self.timer.elapsed(),
                                                    );

                                                    let sent_event =
                                                        Event::SentPacket { sent: packet };
                                                    new_events.push(sent_event);
                                                    continue;
                                                }
                                                Err(fail)
                                                    if fail.kind() == io::ErrorKind::WouldBlock =>
                                                {
                                                    warn!("Would block on send_to {:?}", fail);
                                                    break 'sender;
                                                }
                                                Err(fail) => {
                                                    error!(
                                                    "Server failed sending packet {:#?} to {:#?}.",
                                                    fail, destination
                                                );
                                                    let failed_event = Event::FailedSendingPacket {
                                                        queued: queued.clone(),
                                                    };
                                                    new_events.push(failed_event);
                                                    break 'sender;
                                                }
                                            }
                                        } else {
                                            error!(
                                            "Trying to send connection accepted to invalid host."
                                        );
                                            unreachable!();
                                        }
                                    }
                                    PacketKind::ConnectionDenied => {}
                                    PacketKind::Ack(_) => {}
                                    PacketKind::Heartbeat => {}
                                    invalid => {
                                        error!(
                                        "Trying to send invalid antarc system packet {:#?} {:#?}",
                                        queued, invalid
                                    );
                                        unreachable!();
                                    }
                                }
                            }

                            self.kind.last_antarc_queue_check = self.timer.elapsed();
                        }

                        if let Some((send_to, queued, payload)) = self.user_queue.first() {
                            debug_assert_ne!(queued.kind, PacketKind::ConnectionRequest);
                            match (send_to, queued.kind) {
                                (SendTo::All, PacketKind::DataTransfer) => {
                                    debug!("Sending data transfer to all.");

                                    let payload_length = payload.len() as u16;

                                    let not_updated = self
                                        .kind
                                        .connected
                                        .iter_mut()
                                        // NOTE(alex) 2021-05-19: Avoid sending this packet to the
                                        // same host multiple times.
                                        .filter(|host| host.state.last_sent < queued.id);

                                    for connected in not_updated {
                                        let sequence = connected.sequence_tracker;
                                        let ack = connected.ack_tracker;
                                        let connection_id = connected.state.connection_id;
                                        let destination = connected.address;
                                        let header = Header {
                                            sequence,
                                            ack,
                                            past_acks: 0,
                                            status_code: DATA_TRANSFER,
                                            payload_length,
                                        };
                                        let (bytes, footer) = match Packet::encode(
                                            &payload,
                                            &header,
                                            Some(connection_id),
                                        ) {
                                            Ok(encoded) => encoded,
                                            Err(fail) => {
                                                error!("Failed encoding packet {:#?}.", fail);
                                                new_events.push(Event::FailedEncodingPacket {
                                                    queued: queued.clone(),
                                                });
                                                break 'sender;
                                            }
                                        };

                                        // TODO(alex) 2021-05-23: Right here, the `bytes` belong to
                                        // a specific Host, so the ownership of this allocation has
                                        // a clear owner. When we fail to send, the encoding is
                                        // performed again, even though we could cache it here as
                                        // a `Packet<Encoded>` and insert it into the Host.

                                        match self.network.udp_socket.send_to(&bytes, destination) {
                                            Ok(num_sent) => {
                                                debug!(
                                                    "Server sent packet {:#?} to {:#?}.",
                                                    queued, destination
                                                );
                                                debug_assert!(num_sent > 0);

                                                let sent = Sent {
                                                    header,
                                                    footer,
                                                    destination,
                                                    time: self.timer.elapsed(),
                                                };
                                                let packet = Packet {
                                                    id: queued.id,
                                                    state: sent,
                                                    kind: queued.kind,
                                                };
                                                let sent_event = Event::SentPacket { sent: packet };
                                                new_events.push(sent_event);
                                                continue;
                                            }
                                            Err(fail)
                                                if fail.kind() == io::ErrorKind::WouldBlock =>
                                            {
                                                warn!("Would block on send_to {:?}", fail);
                                                break 'sender;
                                            }
                                            Err(fail) => {
                                                error!(
                                                    "Server failed sending packet {:#?} to {:#?}.",
                                                    fail, destination
                                                );
                                                let failed_event = Event::FailedSendingPacket {
                                                    queued: queued.clone(),
                                                };
                                                new_events.push(failed_event);
                                                break 'sender;
                                            }
                                        }
                                    }
                                }

                                invalid => {
                                    error!(
                                        "Trying to send to with an invalid combination {:#?}",
                                        invalid
                                    );
                                    unreachable!();
                                }
                            }
                        } else {
                            // TODO(alex) 2021-05-18: No packets queued, send heartbeat!
                        }
                    }
                }
                Event::SendConnectionRequest { address } => {
                    error!("Server cannot handle SendConnectionRequest!");
                    unreachable!();
                }
                Event::ReceivedConnectionRequest { address } => {
                    debug!("Handling ReceivedConnectionRequest from {:#?}", address);
                }
                Event::FailedEncodingPacket { queued } => {
                    error!("Handling event FailedEncodingPacket for {:#?}", queued);
                }
                Event::FailedSendingPacket { queued } => {
                    error!("Handling event FailedSendingPacket for {:#?}", queued);
                }
                Event::SentPacket { sent } => {
                    debug!("Handling SentPacket for {:#?}", sent);
                    if self
                        .kind
                        .connected
                        .iter()
                        .all(|host| host.state.last_sent == sent.id)
                    {
                        let removed = self
                            .user_queue
                            .drain_filter(|(_, packet, _)| packet.id == sent.id)
                            .next();
                        debug!("Removed {:#?} from queue.", removed);
                    }
                }
                Event::ReceivedPacket { received } => {
                    debug!("Handling ReceivedPacket for {:#?}", received);
                }
                Event::SendHeartbeat { address } => {
                    debug!("Handling SendHeartbeat to {:#?}", address);
                }
            }
        }

        self.events.append(&mut new_events);
        Ok(self.kind.received_tracker)
    }

    pub fn retrieve(&mut self) -> Vec<(ConnectionId, Vec<u8>)> {
        debug!("Retrieve for server.");
        self.kind.received_tracker = 0;
        Vec::with_capacity(4)
    }
}
