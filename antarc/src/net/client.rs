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
use crate::{MTU_LENGTH, event::{Event, EventKind}, host::{Disconnected, Host, HostState}, net::{NetManager, NetworkResource}, packet::{
        header::Header, ConnectionId, Encoded, Footer, Packet, Payload, Queued, Received, Sequence,
        CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST, DATA_TRANSFER, HEARTBEAT,
    }};

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
    pub(crate) queued: Vec<Packet<Queued>>,
    pub(crate) received: Vec<Packet<Received>>,
    pub(crate) server: Option<Host>,
}

fn receiver(mut buffer: &mut [u8], socket: &UdpSocket, timer: &Instant) -> Vec<Event> {
    let mut new_events = Vec::with_capacity(8);
    loop {
        match socket.recv_from(&mut buffer) {
            Ok((num_received, from_address)) => {
                debug_assert!(num_received > 0);
                let received = Packet::decode(&buffer[..num_received], timer).unwrap();
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

    new_events
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
        if let Some(server) = &self.client_or_server.server {
            if server.disconnected() {
                self.events.push(Event::SendConnectionRequest {
                    address: server_addr.clone(),
                });
            } else {
                error!("Host is in incorrect state to start requesting connection.");
            }
        } else {
            let state = HostState::Disconnected;
            let server = Host::new(server_addr.clone(), state);
            self.client_or_server.server = Some(server);
                self.events.push(Event::SendConnectionRequest {
                    address: server_addr.clone(),
                });
        }
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
        let mut writable = false;

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
                Event::QueuedPacket { queued: packet } => {
                    self.client_or_server.queued.push(packet);
                }
                Event::ReadyToReceive => {}
                Event::ReadyToSend => {
                    debug!("Client ready to send.");

                    loop {
                        if let Some(queued) = self.client_or_server.queued.pop() {
                            let header = Header::default();
                            let (bytes, footer) = match queued.encode(&header, None) {
                                Ok(success) => success,
                                Err(fail) => {
                                    error!("Failed encoding packet {:#?}.", fail);
                                    new_events.push(Event::FailedEncodingPacket { queued });
                                    continue;
                                }
                            };
                            let encoded = queued.to_encoded(header, footer, &self.timer);
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
                                    // TODO(alex) 2021-05-17: Must treat 2 different errors here,
                                    // the WouldBlock and socket issues, each will have its own
                                    // handling mechanism.
                                    break;
                                }
                            }
                        } else {
                            // TODO(alex) 2021-05-17: Send heartbeat.
                            todo!()
                        }
                    }
                }
                Event::FailedEncodingPacket { queued: packet } => {}
                Event::FailedSendingPacket { encoded: packet } => {}
                Event::SentPacket { sent: packet } => {}
                Event::ReceivedPacket { received: packet } => {}
                Event::SendConnectionRequest { address } => {
                    let header = Header {
                        sequence: Sequence::one(),
                        status_code: CONNECTION_REQUEST,
                        ..Default::default()
                    };
                    let state = Queued {
                        time: self.timer.elapsed(),
                    };
                    let payload = Payload::default();
                    let packet = Packet {
                        payload,
                        state,
                        address,
                    };

                    new_events.push(Event::QueuedPacket { queued: packet });
                }
                Event::ReceivedConnectionRequest { address } => {}
            }
        }
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
