use std::{
    io,
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
    time::{Duration, Instant},
};

use log::{debug, error, warn};

use super::SendTo;
use crate::{
    events::CommonEvent,
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

    // TODO(alex) 2021-05-23: Allow the user to specify the destination of these messages, we then
    // check if they're in the `connected` list, and queue the packets as normal.
    pub fn enqueue(&mut self, message: Vec<u8>) -> PacketId {
        let id = self.kind.id_tracker;

        for host in self.kind.connected.iter() {
            let state = Queued {
                time: self.timer.elapsed(),
                destination: host.address,
            };
            let packet = Packet {
                id,
                state,
                kind: PacketKind::DataTransfer,
            };

            self.user_queue.push(packet);
        }

        self.payload_queue.insert(id, Payload(message));
        self.kind.id_tracker += 1;

        id
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
                self.network.readable = true;
            }

            if event.is_writable() {
                self.network.writable = true;
            }
        }

        for event in self.events.drain(..) {
            match event {
                CommonEvent::ReadyToReceive => {
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
                                new_events.push(CommonEvent::ReceivedPacket { received });
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
                // TODO(alex) [high] 2021-05-23: We don't depend on these events anymore, now we
                // should just check if `self.network.writable` and try to send the packet, this
                // allows us to cover sending by events, instead of send by queued vec.
                CommonEvent::ReadyToSend => {
                    debug!("Handling ReadyToSend");

                    if self.user_queue.is_empty()
                        || self.timer.elapsed()
                            > CHECK_ANTARC_QUEUE + self.kind.last_antarc_queue_check
                    {
                        for queued in self.antarc_queue.iter() {
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
                                        .find(|host| host.address == queued.state.destination)
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
                                                new_events.push(
                                                    CommonEvent::FailedEncodingPacket {
                                                        queued: queued.clone(),
                                                    },
                                                );
                                                break;
                                            }
                                        };
                                        match self.network.udp_socket.send_to(&bytes, destination) {
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
                                                    CommonEvent::SentPacket { sent: packet };
                                                new_events.push(sent_event);
                                            }
                                            Err(fail)
                                                if fail.kind() == io::ErrorKind::WouldBlock =>
                                            {
                                                warn!("Would block on send_to {:?}", fail);
                                                self.network.writable = false;
                                                break;
                                            }
                                            Err(fail) => {
                                                error!(
                                                    "Server failed sending packet {:#?} to {:#?}.",
                                                    fail, destination
                                                );
                                                let failed_event =
                                                    CommonEvent::FailedSendingPacket {
                                                        queued: queued.clone(),
                                                    };
                                                new_events.push(failed_event);
                                                break;
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
                                PacketKind::DataTransfer => {}
                                PacketKind::Heartbeat => {}
                            }
                        }

                        self.kind.last_antarc_queue_check = self.timer.elapsed();
                    } else {
                        for queued in self.user_queue.iter() {
                            debug_assert_ne!(queued.kind, PacketKind::ConnectionRequest);
                            debug!("Sending data transfer from the user queue.");

                            let payload = self.payload_queue.get(&queued.id).unwrap();
                            let payload_length = payload.len() as u16;

                            let connected = self
                                .kind
                                .connected
                                .iter()
                                .find(|host| host.address == queued.state.destination)
                                .unwrap();

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
                            let (bytes, footer) =
                                match Packet::encode(&payload, &header, Some(connection_id)) {
                                    Ok(encoded) => encoded,
                                    Err(fail) => {
                                        error!("Failed encoding packet {:#?}.", fail);
                                        new_events.push(CommonEvent::FailedEncodingPacket {
                                            queued: queued.clone(),
                                        });
                                        break;
                                    }
                                };

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
                                    let sent_event = CommonEvent::SentPacket { sent: packet };
                                    new_events.push(sent_event);
                                }
                                Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                                    warn!("Would block on send_to {:?}", fail);

                                    // TODO(alex) 2021-05-23: Right here, the `bytes` belongs to a
                                    // specific Host, so the ownership of this allocation has a
                                    // clear owner. When we fail
                                    // to send, the encoding is performed again,
                                    // even though we could cache it here as a `Packet<Encoded>` and
                                    // insert it into the Host.
                                    self.network.writable = false;

                                    break;
                                }
                                Err(fail) => {
                                    error!(
                                        "Server failed sending packet {:#?} to {:#?}.",
                                        fail, destination
                                    );
                                    let failed_event = CommonEvent::FailedSendingPacket {
                                        queued: queued.clone(),
                                    };
                                    new_events.push(failed_event);
                                    break;
                                }
                            }
                        }
                    }
                }
                CommonEvent::SendConnectionRequest { address } => {
                    error!("Server cannot handle SendConnectionRequest!");
                    unreachable!();
                }
                CommonEvent::ReceivedConnectionRequest { address } => {
                    debug!("Handling ReceivedConnectionRequest from {:#?}", address);
                }
                CommonEvent::FailedEncodingPacket { queued } => {
                    error!("Handling event FailedEncodingPacket for {:#?}", queued);
                }
                CommonEvent::FailedSendingPacket { queued } => {
                    error!("Handling event FailedSendingPacket for {:#?}", queued);
                }
                CommonEvent::SentPacket { sent } => {
                    debug!("Handling SentPacket for {:#?}", sent);
                    if self
                        .kind
                        .connected
                        .iter()
                        .all(|host| host.state.last_sent >= sent.id)
                    {
                        let removed_payload = self.payload_queue.remove(&sent.id);
                        debug!("Removed {:#?} from queue.", removed_payload);
                    }

                    let removed_packet = self.user_queue.drain(..1).next();
                    debug!("Removed {:#?} from queue.", removed_packet);
                }
                CommonEvent::ReceivedPacket { received } => {
                    debug!("Handling ReceivedPacket for {:#?}", received);
                }
                CommonEvent::SendHeartbeat { address } => {
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
