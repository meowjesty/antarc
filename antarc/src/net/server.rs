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
    net::{NetManager, NetworkResource},
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

            // self.user_queue.push(packet);
            self.events.push(CommonEvent::QueuedDataTransfer { packet });
        }

        self.payload_queue.insert(id, Payload(message));
        self.kind.id_tracker += 1;

        id
    }

    // TODO(alex) 2021-05-17: It's probably a good idea to start working on this before going
    // further, to validate that the ideas I had so far are working. Make this the focus.
    pub fn tick(&mut self) -> Result<usize, String> {
        let mut new_events = Vec::with_capacity(8);
        let mut handled_events = Vec::with_capacity(8);

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

        while self.network.readable {
            match self.network.udp_socket.recv_from(&mut self.buffer) {
                Ok((num_received, source)) => {
                    debug!("Received new packet {:#?} {:#?}.", num_received, source);
                    debug_assert!(num_received > 0);
                    let received = Packet::decode(
                        self.kind.id_tracker,
                        &self.buffer[..num_received],
                        source,
                        &self.timer,
                    )
                    .unwrap();

                    self.events.push(CommonEvent::ReceivedPacket { received });
                    self.kind.id_tracker += 1;
                    self.retrievable_count += 1;
                }
                Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                    warn!("Would block on recv_from {:?}", fail);
                    self.network.readable = false;
                }
                Err(fail) => {
                    warn!("Failed recv_from with {:?}", fail);
                    self.network.readable = false;
                }
            }
        }

        for (event_id, event) in self.events.iter().enumerate() {
            match event {
                CommonEvent::QueuedDataTransfer { packet } if self.network.writable => {
                    debug!("Handling QueuedDataTransfer {:#?}.", packet);

                    let payload = self.payload_queue.get(&packet.id).unwrap();
                    let payload_length = payload.len() as u16;

                    let connected = self
                        .kind
                        .connected
                        .iter_mut()
                        .find(|host| host.address == packet.state.destination)
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
                                    queued: packet.clone(),
                                });
                                handled_events.push(event_id);
                                break;
                            }
                        };

                    match self.network.udp_socket.send_to(&bytes, destination) {
                        Ok(num_sent) => {
                            debug!("Server sent packet {:#?} to {:#?}.", packet, destination);
                            debug_assert!(num_sent > 0);

                            let sent = Sent {
                                header,
                                footer,
                                destination,
                                time: self.timer.elapsed(),
                            };
                            let packet = Packet {
                                id: packet.id,
                                state: sent,
                                kind: packet.kind,
                            };

                            let sent_event = CommonEvent::SentPacket { sent: packet };
                            connected.sequence_tracker =
                                Sequence::new(connected.sequence_tracker.get() + 1).unwrap();
                            new_events.push(sent_event);
                            handled_events.push(event_id);
                        }
                        Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                            warn!("Would block on send_to {:?}", fail);

                            // TODO(alex) [low] 2021-05-23: Right here, the `bytes` belongs to a
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
                                queued: packet.clone(),
                            };
                            new_events.push(failed_event);
                            handled_events.push(event_id);
                            break;
                        }
                    }
                }
                CommonEvent::QueuedConnectionAccepted { packet } if self.network.writable => {
                    debug!("Handling QueuedConnectionAccepted {:#?}.", packet);

                    let payload = Payload::default();
                    let payload_length = payload.len() as u16;

                    let requesting_connection = self
                        .kind
                        .requesting_connection
                        .iter()
                        .find(|host| host.address == packet.state.destination)
                        .unwrap();

                    let sequence = Sequence::default();
                    let ack = 1;
                    let connection_id = self.kind.connection_id_tracker;
                    let destination = requesting_connection.address;
                    let header = Header {
                        sequence,
                        ack,
                        past_acks: 0,
                        status_code: CONNECTION_ACCEPTED,
                        payload_length,
                    };
                    let (bytes, footer) =
                        match Packet::encode(&payload, &header, Some(connection_id)) {
                            Ok(encoded) => encoded,
                            Err(fail) => {
                                error!("Failed encoding packet {:#?}.", fail);
                                new_events.push(CommonEvent::FailedEncodingPacket {
                                    queued: packet.clone(),
                                });
                                handled_events.push(event_id);
                                break;
                            }
                        };

                    match self.network.udp_socket.send_to(&bytes, destination) {
                        Ok(num_sent) => {
                            debug!("Server sent packet {:#?} to {:#?}.", packet, destination);
                            debug_assert!(num_sent > 0);

                            let sent = Sent {
                                header,
                                footer,
                                destination,
                                time: self.timer.elapsed(),
                            };
                            let packet = Packet {
                                id: packet.id,
                                state: sent,
                                kind: packet.kind,
                            };
                            let sent_event = CommonEvent::SentPacket { sent: packet };
                            self.kind.connection_id_tracker =
                                NonZeroU16::new(self.kind.connection_id_tracker.get() + 1).unwrap();
                            new_events.push(sent_event);
                            handled_events.push(event_id);
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
                                queued: packet.clone(),
                            };
                            new_events.push(failed_event);
                            handled_events.push(event_id);
                            break;
                        }
                    }
                }
                CommonEvent::QueuedConnectionRequest { .. } => {
                    error!("Server cannot handle SendConnectionRequest!");
                    unreachable!();
                }
                CommonEvent::ReceivedConnectionRequest { address } => {
                    debug!("Handling ReceivedConnectionRequest from {:#?}", address);
                    handled_events.push(event_id);
                }
                CommonEvent::FailedEncodingPacket { queued } => {
                    error!("Handling event FailedEncodingPacket for {:#?}", queued);
                    handled_events.push(event_id);
                }
                CommonEvent::FailedSendingPacket { queued } => {
                    error!("Handling event FailedSendingPacket for {:#?}", queued);
                    handled_events.push(event_id);
                }
                CommonEvent::SentPacket { sent } => {
                    debug!("Handling SentPacket for {:#?}", sent);

                    match sent.kind {
                        PacketKind::ConnectionRequest => {
                            error!("Server cannot have sent a connection request {:#?}.", sent);
                            unreachable!();
                        }
                        PacketKind::ConnectionAccepted => {
                            debug!("Server sent ConnectionAccepted, no payload to remove.");
                        }
                        PacketKind::ConnectionDenied => {
                            debug!("Server sent ConnectionDenied.");
                        }
                        PacketKind::Ack(_) => {}
                        PacketKind::DataTransfer => {
                            debug!("Server sent DataTransfer.");

                            if self
                                .kind
                                .connected
                                .iter()
                                .all(|host| host.state.last_sent >= sent.id)
                            {
                                let removed_payload = self.payload_queue.remove(&sent.id);
                                debug!("Removed {:#?} from queue.", removed_payload);
                            }
                        }
                        PacketKind::Heartbeat => {}
                    }

                    handled_events.push(event_id);
                }
                CommonEvent::ReceivedPacket { received } => {
                    debug!("Handling ReceivedPacket for {:#?}", received);
                    handled_events.push(event_id);
                }
                CommonEvent::QueuedHeartbeat { address } => {
                    debug!("Handling SendHeartbeat to {:#?}", address);
                    handled_events.push(event_id);
                }
                other => {
                    warn!("Server catch all for improper event {:#?}.", other);
                }
            }
        }

        for handled_event in handled_events.drain(..) {
            let removed_event = self.events.remove(handled_event);
            debug!("Removed event {:#?}.", removed_event);
        }

        self.events.append(&mut new_events);
        Ok(self.retrievable_count)
    }

    pub fn retrieve(&mut self) -> Vec<(ConnectionId, Vec<u8>)> {
        debug!("Retrieve for server.");
        self.retrievable_count = 0;
        Vec::with_capacity(4)
    }
}
