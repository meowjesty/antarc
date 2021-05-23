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
    events::{Event, EventKind},
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
        let state = Queued {
            time: self.timer.elapsed(),
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
        self.antarc_queue.push((*server_addr, packet));
        self.kind.id_tracker += 1;

        self.events.push(Event::SendConnectionRequest {
            address: server_addr.clone(),
        });
    }

    pub fn enqueue(&mut self, message: Vec<u8>) -> u64 {
        let id = self.kind.id_tracker;
        let address = self.kind.server.address;
        let state = Queued {
            time: self.timer.elapsed(),
        };
        let packet = Packet {
            id,
            state,
            kind: PacketKind::DataTransfer,
        };

        self.user_queue
            .push((SendTo::Single(address), packet, Payload(message)));
        self.kind.id_tracker += 1;

        id
    }

    pub fn cancel_packet(&mut self, packet_id: u64) -> bool {
        self.user_queue
            .drain_filter(|(_, queued, _)| queued.id == packet_id)
            .next()
            .is_some()
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

        self.network
            .poll
            .poll(&mut self.network.events, Some(Duration::from_millis(150)))
            .unwrap();

        for event in self.network.events.iter() {
            if event.token() == NetworkResource::TOKEN {
                if event.is_readable() {
                    self.events.push(Event::ReadyToReceive);
                }

                if event.is_writable() {
                    self.events.push(Event::ReadyToSend);
                }
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

                    loop {
                        let sequence = self.kind.server.sequence_tracker;

                        let ack = self.kind.server.ack_tracker;

                        let connection_id = self.kind.server.state.state.connection_id();
                        let address = self.kind.server.address;

                        if let Some((send_to, queued, payload)) = self.user_queue.pop() {
                            if let SendTo::Single(address) = send_to {
                                let status_code = From::from(queued.kind);

                                let payload_length = payload.len() as u16;

                                let header = Header {
                                    sequence,
                                    ack,
                                    past_acks: 0,
                                    status_code,
                                    payload_length,
                                };

                                let (bytes, footer) =
                                    match Packet::encode(&payload, &header, connection_id) {
                                        Ok(success) => success,
                                        Err(fail) => {
                                            error!("Failed encoding packet {:#?}.", fail);
                                            new_events.push(Event::FailedEncodingPacket { queued });
                                            continue;
                                        }
                                    };

                                match sender(
                                    &self.network.udp_socket,
                                    &bytes,
                                    &address,
                                    &self.timer,
                                ) {
                                    Ok(_) => {
                                        debug!("Client sent packet successfully {:#?}.", queued);
                                        let sent = Sent {
                                            header,
                                            footer,
                                            destination: address,
                                            time: self.timer.elapsed(),
                                        };
                                        let packet = Packet {
                                            id: queued.id,
                                            state: sent,
                                            kind: queued.kind,
                                        };
                                        new_events.push(Event::SentPacket { sent: packet });
                                    }
                                    Err(fail) => {
                                        error!("Client failed sending packet {:#?}.", fail);
                                        new_events.push(Event::FailedSendingPacket { queued });
                                        // TODO(alex) 2021-05-17: Must treat 2 different errors
                                        // here,
                                        // the WouldBlock and socket issues, each will have its own
                                        // handling mechanism.
                                        break;
                                    }
                                }
                            } else {
                                error!("Invalid send to {:#?} for client.", send_to);
                                unreachable!();
                            }
                        } else {
                            // TODO(alex) 2021-05-18: Only send this heartbeat if we received some
                            // packet from the server, and we have not sent an ack for it yet.
                            new_events.push(Event::SendHeartbeat { address });
                        }
                    }
                }
                Event::FailedEncodingPacket { queued } => {
                    error!("Handling event FailedEncodingPacket for {:#?}", queued);
                }
                Event::FailedSendingPacket { queued } => {
                    error!("Handling event FailedSendingPacket for {:#?}", queued);
                }
                Event::SentPacket { sent } => {
                    debug!("Handling SentPacket for {:#?}", sent);
                    let removed = self
                        .user_queue
                        .drain_filter(|(_, packet, _)| packet.id == sent.id)
                        .next();
                    debug!("Removed {:#?} from queue.", removed);
                }
                Event::ReceivedPacket { received } => {
                    debug!("Handling ReceivedPacket for {:#?}", received);
                    // TODO(alex) 2021-05-17: What to do here before this?
                    self.kind.server.received.push(received);
                }
                Event::SendConnectionRequest { address } => {
                    unreachable!();
                }
                Event::SendHeartbeat { address } => {
                    // TODO(alex) 2021-05-18: Both here and on the server we should check the rtt
                    // to see if a hearbeat is actually neccessary, thus
                    // avoiding a network congestion.
                    let id = self.kind.id_tracker;
                    let sequence = self.kind.server.sequence_tracker;
                    let ack = self.kind.server.ack_tracker;
                    let status_code = HEARTBEAT;
                    let payload = Payload::default();
                    let payload_length = payload.len() as u16;

                    let header = Header {
                        sequence,
                        ack,
                        past_acks: 0,
                        status_code,
                        payload_length,
                    };

                    let state = Queued {
                        time: self.timer.elapsed(),
                    };
                    let kind = PacketKind::Heartbeat;

                    let address = self.kind.server.address;

                    let packet = Packet { id, state, kind };

                    self.user_queue
                        .push((SendTo::Single(address), packet, Payload::default()));
                    self.kind.id_tracker += 1;
                }
                Event::ReceivedConnectionRequest { address } => {
                    error!("Client cannot handle ReceivedConnectionRequest!");
                    unreachable!();
                }
            }
        }

        self.events.append(&mut new_events);

        Ok(self.kind.server.received.len())
    }

    /// NOTE(alex): This is less of a system, and more just a function that the user will call,
    /// part of the public API (exposed via `NetManager` client / server).
    pub fn retrieve(&mut self) -> Vec<(ConnectionId, Vec<u8>)> {
        debug!("Retrieving packets for {:#?}", self);
        Vec::with_capacity(4)
    }
}
