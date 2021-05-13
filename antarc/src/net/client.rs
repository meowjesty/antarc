use std::{io, net::SocketAddr, sync::Arc, time::Duration};

use hecs::Entity;
use log::{debug, error, warn};

use crate::{
    event::Event,
    host::{Disconnected, Host},
    net::{NetManager, NetworkResource},
    packet::{
        header::Header, ConnectionAccepted, ConnectionId, ConnectionRequest, DataTransfer, Footer,
        Heartbeat, Internal, Packet, Payload, Queued, Received, Retrieved, Sent, Sequence,
        CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST, DATA_TRANSFER, HEARTBEAT,
    },
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
    server: Option<Host>,
}

/// TODO(alex) 2021-04-24: `LatestReceived` makes sense to be part of the `Host` entity, this
/// way we can do `world.insert` and not have to remove and then insert (packet case).
#[derive(Debug)]
pub(crate) struct LastReceived {
    packet_id: Entity,
}

#[derive(Debug)]
pub(crate) struct LastSent {
    packet_id: Entity,
}

#[derive(Debug)]
pub(crate) struct LastAcked {
    packet_id: Entity,
}

impl NetManager<Client> {
    pub fn new_client(address: &SocketAddr) -> Self {
        let client = Client { server: None };
        let net_manager = NetManager::new(address, client);
        net_manager
    }

    /// TODO(alex) 2021-02-26: Authentication is something that we can't do here, it's up to the
    /// user, but there must be an API for forcefully dropping a connection, plus banning a host.
    pub fn connect(&mut self, server_addr: &SocketAddr) -> () {
        let header = Header {
            sequence: Sequence::one(),
            status_code: CONNECTION_REQUEST,
            ..Default::default()
        };
        let payload = Payload::default();
        let connection_request = Queued {
            header,
            payload,
            time: self.timer.elapsed(),
        };

        // TODO(alex): How do I insert the queued packet here? Do I even want to insert it here?
        // Or just raise the queued packet event, and handle it somewhere else???
        let mut host = Host::disconnected(*server_addr);
        host.enqueue(connection_request);

        let server = Arc::new(host);
        self.events
            .push(Event::QueuedEvent(Arc::downgrade(&server)));
    }

    fn receiver(&mut self) {
        loop {
            match self.network_resource.udp_socket.recv_from(&mut self.buffer) {
                Ok((num_received, from_address)) => {
                    debug_assert!(num_received > 0);
                    self.events.push(Event::ReceivedEvent((
                        self.buffer[0..num_received].to_vec(),
                        from_address,
                    )));
                }
                Err(fail) => {
                    error!("Failed receiving with {fail}.");
                    todo!();
                    break;
                }
            }
        }
    }

    fn sender(&mut self, queued: &Queued, destination: &SocketAddr) {
        // TODO(alex) 2021-05-12: This requires going back to the drawing board...
        let (encoded, footer) = Packet::encode(&queued.header, &queued.payload, None).unwrap();
        loop {
            match self
                .network_resource
                .udp_socket
                .send_to(&encoded, *destination)
            {
                Ok(num_sent) => {}
                Err(fail) => {
                    error!("Failed sending with {fail}.");
                    todo!();
                    break;
                }
            }
        }
    }

    fn readiness(&mut self) {
        let resource = &mut self.network_resource;
        resource
            .poll
            .poll(&mut resource.events, Some(Duration::from_millis(150)))
            .unwrap();

        for event in resource.events.iter() {
            if event.token() == NetworkResource::TOKEN {
                if event.is_readable() {
                    self.events.push(Event::ReadableEvent);
                }

                if event.is_writable() {
                    self.events.push(Event::WritableEvent);
                }
            }
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
        self.network_resource
            .poll
            .poll(
                &mut self.network_resource.events,
                Some(Duration::from_millis(150)),
            )
            .unwrap();

        for event in self.network_resource.events.iter() {
            if event.token() == NetworkResource::TOKEN {
                if event.is_readable() {
                    loop {
                        match self.network_resource.udp_socket.recv_from(&mut self.buffer) {
                            Ok((num_received, source)) => {
                                if let Some(server) = self.client_or_server.server.as_mut() {
                                    debug_assert_eq!(server.info.address, source);
                                    server.on_received(&self.buffer[..num_received], &self.timer);
                                }
                            }
                            Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                                warn!("`recv_from` exhausted readable operation with {fail}.");
                            }
                            Err(fail) => {
                                error!("`recv_from` failed with {fail}.");
                                todo!()
                            }
                        }
                    }
                }

                // TODO(alex) 2021-05-12: Looping to handle the events looks like a good idea here,
                // but it won't work in practice, if we have no packet to send, then it won't ever
                // reach the `WouldBlock` state required to clear the poll events.
                if event.is_writable() {
                    if let Some(host) = self.client_or_server.server.as_mut() {
                        if let Some(packet) = host.pop() {}
                    }
                }
            }
        }
    }

    /// System responsible for attributing a `Source` host to a packet entity, if the host matches
    /// the packet's `Address`, otherwise it raises the `OnReceivedConnectionRequest` event (if
    /// the `Header` represents a `ConnectionRequest`).
    ///
    /// Also handles the changes to a host's `LatestReceived` packet.
    ///
    /// - Raises the `OnReceivedConnectionRequest` event.
    pub(crate) fn on_received_new_packet(&mut self) {}

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
