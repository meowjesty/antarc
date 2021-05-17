use std::{
    io,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use hecs::Entity;
use log::{debug, error, warn};
use mio::net::UdpSocket;

use super::Connection;
use crate::{
    event::{Event, EventKind},
    host::{Disconnected, Host, HostState},
    net::{NetManager, NetworkResource},
    packet::{
        header::Header, ConnectionId, Encoded, Footer, Packet, PacketKind, Payload, Queued,
        Received, Sequence, CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST,
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
#[derive(Debug, Default)]
pub struct Client {
    pub(crate) server: Host,
}

fn receiver(
    mut buffer: &mut [u8],
    socket: &UdpSocket,
    timer: &Instant,
    new_events: &mut Vec<Event>,
) {
    let mut new_events = Vec::with_capacity(8);
    loop {
        match socket.recv_from(&mut buffer) {
            Ok((num_received, from_address)) => {
                debug_assert!(num_received > 0);
                let received =
                    Packet::decode(&buffer[..num_received], from_address, timer).unwrap();
                new_events.push(Event::ReceivedPacket { received });
            }
            Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                warn!("Would block on recv_from {:?}", fail);
                break;
            }
            Err(fail) => {
                error!("Failed receiving with {:#?}.", fail);
                todo!();
                break;
            }
        }
    }
}

fn sender(
    socket: &UdpSocket,
    encoded: &Packet<Encoded>,
    destination: &SocketAddr,
    timer: &Instant,
) -> Result<(), String> {
    match socket.send_to(&encoded.state.bytes, *destination) {
        Ok(num_sent) => {
            debug_assert!(num_sent > 0);
            Ok(())
        }
        Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
            warn!("Would block on send_to {:?}", fail);
            Err(fail.to_string())
        }
        Err(fail) => {
            error!("Failed sending with {:#?}.", fail);
            todo!();
        }
    }
}

impl NetManager<Client> {
    pub fn new_client(address: &SocketAddr) -> Self {
        let client = Client::default();
        let net_manager = NetManager::new(address, client);
        net_manager
    }

    /// TODO(alex) 2021-02-26: Authentication is something that we can't do here, it's up to the
    /// user, but there must be an API for forcefully dropping a connection, plus banning a host.
    pub fn connect(&mut self, server_addr: &SocketAddr) -> () {
        let state = HostState::Disconnected;
        let server = Host::new(server_addr.clone(), state);
        self.kind.server = server;
        self.events.push(Event::SendConnectionRequest {
            address: server_addr.clone(),
        });
    }

    pub fn enqueue(&mut self, message: Vec<u8>) {}

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
    pub fn tick(&mut self) -> () {
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
                Event::QueuedPacket { queued } => {
                    debug!("Handling QueuedPacket for {:#?}", queued);
                    // TODO(alex) 2021-05-17: What to do here before this?
                    self.kind.server.queued.push(queued);
                }
                Event::ReadyToReceive => {
                    debug!("Handling ReadyToReceive");
                    receiver(
                        &mut self.buffer,
                        &self.network.udp_socket,
                        &self.timer,
                        &mut new_events,
                    );
                }
                Event::ReadyToSend => {
                    debug!("Handling ReadyToSend");

                    loop {
                        let sequence = self
                            .kind
                            .server
                            .sent
                            .last()
                            .map(|sent| sent.state.header.sequence)
                            .unwrap_or_default();

                        let ack = self
                            .kind
                            .server
                            .received
                            .last()
                            .map(|received| received.state.header.sequence.get())
                            .unwrap_or_default();

                        let connection_id = self.kind.server.state.connection_id();

                        let encoded = match self.kind.server.queued.pop() {
                            Some(queued) => {
                                let status_code = From::from(queued.kind);

                                let payload_length = queued.payload.len() as u16;

                                let header = Header {
                                    sequence,
                                    ack,
                                    past_acks: 0,
                                    status_code,
                                    payload_length,
                                };

                                let (bytes, footer) = match queued.encode(&header, connection_id) {
                                    Ok(success) => success,
                                    Err(fail) => {
                                        error!("Failed encoding packet {:#?}.", fail);
                                        new_events.push(Event::FailedEncodingPacket { queued });
                                        continue;
                                    }
                                };
                                let encoded = queued.to_encoded(header, footer, &self.timer);
                                encoded
                            }
                            None => {
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

                                let packet = Packet {
                                    payload,
                                    state,
                                    kind,
                                    address,
                                };

                                let (bytes, footer) = match packet.encode(&header, connection_id) {
                                    Ok(success) => success,
                                    Err(fail) => {
                                        error!("Failed encoding packet {:#?}.", fail);
                                        new_events
                                            .push(Event::FailedEncodingPacket { queued: packet });
                                        continue;
                                    }
                                };
                                let encoded = packet.to_encoded(header, footer, &self.timer);
                                encoded
                            }
                        };

                        match sender(
                            &self.network.udp_socket,
                            &encoded,
                            &encoded.address,
                            &self.timer,
                        ) {
                            Ok(_) => {
                                debug!("Client sent packet successfully {:#?}.", encoded);
                                let sent = encoded.to_sent(&self.timer);
                                new_events.push(Event::SentPacket { sent });
                            }
                            Err(fail) => {
                                error!("Client failed sending packet {:#?}.", fail);
                                new_events.push(Event::FailedSendingPacket { encoded });
                                // TODO(alex) 2021-05-17: Must treat 2 different errors
                                // here,
                                // the WouldBlock and socket issues, each will have its own
                                // handling mechanism.
                                break;
                            }
                        }
                    }
                }
                Event::FailedEncodingPacket { queued } => {
                    error!("Handling event FailedEncodingPacket for {:#?}", queued);
                }
                Event::FailedSendingPacket { encoded } => {
                    error!("Handling event FailedSendingPacket for {:#?}", encoded);
                }
                Event::SentPacket { sent } => {
                    debug!("Handling SentPacket for {:#?}", sent);
                    // TODO(alex) 2021-05-17: What to do here before this?
                    self.kind.server.sent.push(sent);
                }
                Event::ReceivedPacket { received } => {
                    debug!("Handling ReceivedPacket for {:#?}", received);
                    // TODO(alex) 2021-05-17: What to do here before this?
                    self.kind.server.received.push(received);
                }
                Event::SendConnectionRequest { address } => {
                    debug!("Handling SendConnectionRequest for {:#?}", address);
                    // TODO(alex) 2021-05-17: This is not 100% correct, as `connect` might not have
                    // been called yet, but not doing this requires `if let Some` pattern in each
                    // event. I have to think of a better way to handle this ordeal.
                    self.kind.server.state = HostState::RequestingConnection { attempts: 0 };

                    let state = Queued {
                        time: self.timer.elapsed(),
                    };
                    let payload = Payload::default();
                    let packet = Packet {
                        payload,
                        state,
                        address,
                        kind: PacketKind::ConnectionRequest,
                    };

                    new_events.push(Event::QueuedPacket { queued: packet });
                }
                Event::ReceivedConnectionRequest { address } => {}
            }
        }

        self.events.append(&mut new_events);
    }

    /// TODO(alex) 2021-03-07: Think of how network libraries usually have a `listen` function,
    /// instead of manually calling `receive`.
    /// NOTE(alex): This is less of a system, and more just a function that the user will call, part
    /// of the public API (exposed via `NetManager` client / server).
    pub fn retrieve(&mut self) -> Vec<(ConnectionId, Vec<u8>)> {
        todo!()
    }

    pub(crate) fn on_received_connection_accepted(&mut self) {
        todo!()
    }

    pub(crate) fn on_received_connection_denied(&mut self) {
        todo!()
    }

    pub(crate) fn on_sent_packet(&mut self) {
        todo!()
    }

    pub(crate) fn on_sent_connection_request(&mut self) {
        todo!()
    }

    pub(crate) fn poll_requesting_connection(&mut self) {
        todo!()
    }

    pub(crate) fn poll_awaiting_connection_ack(&mut self) {
        // TODO(alex) 2021-05-02: Check if the connection timed out, and resend the connection
        // request, if the number of attempts is still valid.
    }

    /// TODO(alex) 2021-02-28: Returns the `Packet<ToSend>::Header::sequence` value so that the user
    /// may use it to remove the packet (if wanted). It'll be an API for users that might want to
    /// clear older packets that were never sent, and to allow users to check for packets that were
    /// actually sent.
    ///
    /// ADD(alex) 2021-03-03: This function is `async` when I think about it. The whole idea of
    /// queueing the packets to send, instead of sending them directly comes from the need to check
    /// if the `socket` is writable, so we enqueue the packets instead of just calling
    /// `socket.send` here, creating an async version of `socket.send` basically. `Client::tick`
    /// is very similar to what I think `poll` would look like.
    ///
    /// Using `Future` would simplify the API, as it would remove the need for the `Client::tick`
    /// function, so `Client::connect -> Future<ConnectedClient>` would do the connection handling
    /// that `Client::tick` is doing, while `Client::send` and `Client::receive` would do the rest
    /// (data transfer handling). `Client::receive` acts like the "update" function, as it will be
    /// called all the time (with some user tickrate), being equivalent to a "listen" function.
    pub fn send(&mut self, data: Vec<u8>) -> () {
        todo!()
    }

    pub fn send_priority(&self, data: Vec<u8>) -> () {
        todo!();
    }
}
