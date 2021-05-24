use std::{
    io::{self, Error},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use log::{debug, error, warn};
use mio::net::UdpSocket;

use super::SendTo;
use crate::{
    events::{CommonEvent, EventKind},
    host::{Disconnected, Generic, Host, HostState, RequestingConnection},
    net::{NetManager, NetworkResource},
    packet::{
        header::Header, ConnectionId, Encoded, Footer, Packet, PacketKind, Payload, Queued,
        Received, Sent, Sequence, CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST,
        DATA_TRANSFER, HEARTBEAT,
    },
    MTU_LENGTH,
};

/// TODO(alex) 2021-05-01: Consider adding a `DebugName` component for every entity, such as when
/// doing `world.spawn((format!("Packet {:?}", header), components...))`.

/// TODO(alex) 2021-02-26: References for ideas about connection:
/// http://www.tcpipguide.com/free/t_PPPLinkSetupandPhases.htm
///
/// ADD(alex) 2021-02-26: We need a `Disconnecting` state (link termination phase)?
///
/// TODO(alex) 2021-03-04: Client and Server are different beasts right now, I'm thinking about
/// ways of allowing some sort of peer-to-peer communication, so a `Client` would have to track
/// connection (`Host<State>`) for multiple other clients. To to this we would need something
/// that looks more like the `Server`, and some way to keep one node of the network as the main
/// server? This idea is not clear yet.
#[derive(Debug)]
pub struct Client {
    pub(crate) id_tracker: u64,
    pub(crate) server: Host<Generic>,
}

pub(crate) fn receiver(
    id: u64,
    mut buffer: &mut [u8],
    socket: &UdpSocket,
    timer: &Instant,
) -> Result<Packet<Received>, Error> {
    match socket.recv_from(&mut buffer) {
        Ok((num_received, from_address)) => {
            debug_assert!(num_received > 0);
            let received =
                Packet::decode(id, &buffer[..num_received], from_address, timer).unwrap();
            Ok(received)
        }
        Err(fail) => {
            error!("Failed receiving with {:#?}.", fail);
            Err(fail)
        }
    }
}

pub(crate) fn sender(
    socket: &UdpSocket,
    bytes: &[u8],
    destination: &SocketAddr,
    timer: &Instant,
) -> Result<(), Error> {
    match socket.send_to(&bytes, *destination) {
        Ok(num_sent) => {
            debug_assert!(num_sent > 0);
            Ok(())
        }
        Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
            warn!("Would block on send_to {:?}", fail);
            Err(fail)
        }
        Err(fail) => {
            error!("Failed sending with {:#?}.", fail);
            Err(fail)
        }
    }
}

impl NetManager<Client> {
    pub fn new_client(address: &SocketAddr) -> Self {
        let client = Client {
            id_tracker: 0,
            server: Host::new_local(),
        };
        let net_manager = NetManager::new(address, client);
        net_manager
    }

    /// TODO(alex) 2021-02-26: Authentication is something that we can't do here, it's up to the
    /// user, but there must be an API for forcefully dropping a connection, plus banning a host.
    ///
    /// TODO(alex) 2021-05-18: I see two ways of making this function:
    ///
    /// 1. It uses a busy loop that will duplicate a bunch of code from `tick`, the whole receiving
    /// plus send part, this would be ideal with `async`;
    ///
    /// 2. It stays as-is, and we rely on `tick` to handle the connection, but then the user won't
    /// know much about the connection, we'll be relying on the `retrieve` to return the
    /// `ConnectionId` + the packet payload (which will be done anyway).
    ///
    /// ADD(alex) 2021-05-23: Do the whole connection inside this function, it'll simplify the
    /// `tick` function immensely.
    pub fn connect(&mut self, server_addr: &SocketAddr) {
        let server = Host::new_generic(server_addr.clone());
        self.kind.server = server;

        // TODO(alex) 2021-05-17: This is not 100% correct, as `connect` might not have
        // been called yet, but not doing this requires `if let Some` pattern in each
        // event. I have to think of a better way to handle this ordeal.
        self.kind.server.state.state = HostState::RequestingConnection {
            info: RequestingConnection { attempts: 0 },
        };

        let id = self.kind.id_tracker;
        let destination = server_addr.clone();
        let state = Queued {
            time: self.timer.elapsed(),
            destination,
        };
        let payload = Payload::default();
        let packet = Packet {
            id,
            state,
            kind: PacketKind::ConnectionRequest,
        };
        // TODO(alex) 2021-05-23: Is this idea of a separate queue actually good? We need to handle
        // the packets here first, and this will take out a huge burden of having to check the kind
        // of packet in `tick` for server/client, but what happens if there are a bunch of
        // connection related packets at the same time?
        // self.antarc_queue.push(packet);
        self.kind.id_tracker += 1;

        self.events
            .push(CommonEvent::QueuedConnectionRequest { packet });
    }

    pub fn enqueue(&mut self, message: Vec<u8>) -> u64 {
        let id = self.kind.id_tracker;
        let destination = self.kind.server.address;
        let state = Queued {
            time: self.timer.elapsed(),
            destination,
        };
        let packet = Packet {
            id,
            state,
            kind: PacketKind::DataTransfer,
        };

        self.events.push(CommonEvent::QueuedDataTransfer { packet });
        self.payload_queue.insert(id, Payload(message));
        self.kind.id_tracker += 1;

        id
    }

    fn heartbeat(&mut self) {}

    pub fn connected(&mut self) {}

    pub fn denied(&mut self) {
        todo!()
    }

    /// TODO(alex) 2021-02-23: Return some indication that the manager received new packets and the
    /// user should call `retrieve`.
    /// ADD(alex) 2021-02-26: The return can be made even more general, by having an enum of
    /// possibilities, like `HasMessagesToRetrieve`, `ConnectionLost`. The only success cases I can
    /// think of are `HasMessagesToRetrieve` and `NothingToReport`? But the errors are plenty, like
    /// `ReceivingMessageFromBannedHost`, `FailedToSend`, `FailedToReceive`, `FailedToEncode`, ...
    pub fn tick(&mut self) -> Result<usize, String> {
        let mut new_events = Vec::with_capacity(8);
        let mut handled_events = Vec::with_capacity(8);

        self.network
            .poll
            .poll(&mut self.network.events, Some(Duration::from_millis(150)))
            .unwrap();

        for event in self.network.events.iter() {
            if event.token() == NetworkResource::TOKEN {
                if event.is_readable() {
                    self.network.readable = true;
                }

                if event.is_writable() {
                    self.network.writable = true;
                }
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
                    break;
                }
                Err(fail) => {
                    warn!("Failed recv_from with {:?}", fail);
                    self.network.readable = false;
                    break;
                }
            }
        }

        for (event_id, event) in self.events.iter().enumerate() {
            match event {
                CommonEvent::QueuedDataTransfer { packet } if self.network.writable => {
                    debug!("Handling QueuedDataTransfer {:#?}.", packet);

                    match &self.kind.server.state.state {
                        HostState::Connected { .. } => (),
                        invalid => {
                            warn!("Client has server in state other than connected!");
                            warn!("Skipping this data transfer!");
                            continue;
                        }
                    }

                    let sequence = self.kind.server.sequence_tracker;
                    let ack = self.kind.server.ack_tracker;
                    let connection_id = self.kind.server.state.state.connection_id();
                    let destination = packet.state.destination;

                    let status_code = From::from(packet.kind);
                    let payload = self.payload_queue.get(&packet.id).unwrap();
                    let payload_length = payload.len() as u16;

                    let header = Header {
                        sequence,
                        ack,
                        past_acks: 0,
                        status_code,
                        payload_length,
                    };

                    let (bytes, footer) = match Packet::encode(&payload, &header, connection_id) {
                        Ok(success) => success,
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
                            debug!("Client sent packet {:#?} to {:#?}.", packet, destination);
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
                            self.kind.server.sequence_tracker =
                                Sequence::new(sequence.get() + 1).unwrap();
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
                                "Client failed sending packet {:#?} to {:#?}.",
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
                CommonEvent::QueuedConnectionRequest { packet } if self.network.writable => {
                    debug!("Handling QueuedConnectionRequest to {:#?}.", packet);

                    let sequence = Sequence::default();
                    let ack = 0;
                    let payload = Payload::default();
                    let payload_length = payload.len() as u16;
                    let destination = packet.state.destination;

                    let header = Header {
                        sequence,
                        ack,
                        past_acks: 0,
                        status_code: CONNECTION_REQUEST,
                        payload_length,
                    };
                    let (bytes, footer) = match Packet::encode(&payload, &header, None) {
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
                            debug!("Client sent packet {:#?} to {:#?}.", packet, destination);
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
                        PacketKind::ConnectionRequest => {}
                        PacketKind::ConnectionAccepted => {}
                        PacketKind::ConnectionDenied => {}
                        PacketKind::Ack(_) => {}
                        PacketKind::DataTransfer => {
                            let removed_payload = self.payload_queue.remove(&sent.id).unwrap();
                            debug!("Removed {:#?} from queue.", removed_payload);
                        }
                        PacketKind::Heartbeat => {}
                    }

                    handled_events.push(event_id);
                }
                CommonEvent::ReceivedPacket { received } => {
                    debug!("Handling ReceivedPacket for {:#?}", received);
                    // TODO(alex) 2021-05-17: What to do here before this?
                    self.kind.server.received.push(received.clone());
                    handled_events.push(event_id);
                }

                CommonEvent::QueuedHeartbeat { address } => {
                    // TODO(alex) 2021-05-18: Both here and on the server we should check the rtt
                    // to see if a hearbeat is actually neccessary, thus
                    // avoiding a network congestion.
                    let id = self.kind.id_tracker;
                    let destination = self.kind.server.address;

                    let state = Queued {
                        time: self.timer.elapsed(),
                        destination,
                    };
                    let kind = PacketKind::Heartbeat;
                    let packet = Packet { id, state, kind };

                    self.payload_queue.insert(packet.id, Payload::default());
                    self.user_queue.push(packet);
                    self.kind.id_tracker += 1;
                }
                CommonEvent::ReceivedConnectionRequest { address } => {
                    error!("Client cannot handle ReceivedConnectionRequest!");
                    unreachable!();
                }
                other => {
                    warn!("Handling other event {:#?}", other);
                }
            }
        }

        for handled_event in handled_events.drain(..) {
            let removed_event = self.events.remove(handled_event);
            debug!("Removed event {:#?}.", removed_event);
        }

        self.events.append(&mut new_events);

        Ok(self.kind.server.received.len())
    }

    /// NOTE(alex): This is less of a system, and more just a function that the user will call,
    /// part of the public API (exposed via `NetManager` client / server).
    pub fn retrieve(&mut self) -> Vec<(ConnectionId, Vec<u8>)> {
        debug!("Retrieving packets.");
        Vec::with_capacity(4)
    }
}
