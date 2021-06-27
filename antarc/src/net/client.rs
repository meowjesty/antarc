use std::{
    io::{self, Error},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use antarc_protocol::{
    events::SenderEvent,
    header::Header,
    hosts::Host,
    packets::{
        raw::RawPacket, scheduled::Scheduled, ConnectionId, Packet, CONNECTION_ACCEPTED,
        CONNECTION_DENIED,
    },
    payload::Payload,
    Client, PacketId, Protocol,
};
use log::{debug, error, warn};
use mio::net::UdpSocket;

use super::SendTo;
use crate::{
    net::{NetManager, NetworkResource},
    send, MTU_LENGTH,
};

/// TODO(alex) 2021-02-26: References for ideas about connection:
/// http://www.tcpipguide.com/free/t_PPPLinkSetupandPhases.htm
///
/// TODO(alex) 2021-03-04: Client and Server are different beasts right now, I'm thinking about
/// ways of allowing some sort of peer-to-peer communication, so a `Client` would have to track
/// connection (`Host<State>`) for multiple other clients. To to this we would need something
/// that looks more like the `Server`, and some way to keep one node of the network as the main
/// server? This idea is not clear yet.
///
/// ADD(alex) [low] 2021-05-27: Piling on this idea, we could just have the user create a network
/// with both a `Client` and a `Server`, this isn't a very good idea, and needs more thought.
impl NetManager<Client> {
    pub fn new_client(address: &SocketAddr) -> Self {
        let protocol = Protocol::new_client();
        let net_manager = NetManager::new(address, protocol);
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
    ///
    /// know much about the connection, we'll be relying on the `retrieve` to return the
    /// `ConnectionId` + the packet payload (which will be done anyway).
    ///
    /// ADD(alex) 2021-05-23: Do the whole connection inside this function, it'll simplify the
    /// `tick` function immensely.
    ///
    /// ADD(alex) [mid] 2021-05-28: Either return the events here for the user to treat, or have
    /// a separate function in the API, that passes the errors to the user, I don't like this
    /// approach very much.
    pub fn connect(&mut self, server_addr: &SocketAddr) -> Result<ConnectionId, String> {
        debug!("Connecting to {:#?}.", server_addr);
        let server = Host::starting_connection_request(server_addr.clone());
        self.connection.requesting_connection.insert(0, server);

        let mut time_sent = self.protocol.timer.elapsed();

        let payload = Payload::default();
        let destination = server_addr.clone();
        let header = todo!();

        // let (bytes, footer) = Packet::encode(&payload, &header.info, None);
        // let mut connection_request = Some(bytes.clone());

        // TODO(alex) [vhigh] 2021-06-14: Make this work.
        loop {
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

            'receive: while self.network.readable {
                match self.network.udp_socket.recv_from(&mut self.buffer) {
                    Ok((num_received, source)) => {
                        debug_assert!(num_received > 0);
                    }
                    Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                        warn!("Would block on recv {:?}", fail);
                        self.network.readable = false;
                        break 'receive;
                    }
                    Err(fail) => {
                        warn!("Failed recv with {:?}", fail);
                        self.network.readable = false;
                        break 'receive;
                    }
                }
            }

            if self.network.writable == false {
                continue;
            }
        }
    }

    pub fn schedule(&mut self, message: Vec<u8>) -> PacketId {
        todo!()
    }

    /// TODO(alex) 2021-02-23: Return some indication that the manager received new packets and the
    /// user should call `retrieve`.
    ///
    /// ADD(alex) 2021-02-26: The return can be made even more general, by having an enum of
    /// possibilities, like `HasMessagesToRetrieve`, `ConnectionLost`. The only success cases I can
    /// think of are `HasMessagesToRetrieve` and `NothingToReport`? But the errors are plenty, like
    /// `ReceivingMessageFromBannedHost`, `FailedToSend`, `FailedToReceive`, `FailedToEncode`, ...
    ///
    /// ADD(alex) [mid] 2021-05-28: Either return the events here for the user to treat, or have
    /// a separate function in the API, that passes the errors to the user, I don't like this
    /// approach very much.
    ///
    /// ADD(alex) [mid] 2021-06-06: Use `thiserror` + `?` operator. Requires having a proper
    /// `AntarcError` enum.
    pub fn tick(&mut self) -> Result<usize, String> {
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
                Ok((num_received, source)) => {}
                Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                    warn!("Would block on recv {:?}", fail);
                    self.network.readable = false;
                    break;
                }
                Err(fail) => {
                    warn!("Failed recv with {:?}", fail);
                    self.network.readable = false;
                    break;
                }
            }
        }

        // TODO(alex) [high] 2021-06-06: Finally de-duplicate this code.
        //
        // ADD(alex) [high] 2021-06-08: Trimmed down some of the duplicated code, but this function
        // is still quite big and repetitive.
        while self.network.writable && self.protocol.event_system.sender.len() > 0 {
            for event in self.protocol.event_system.sender.drain(..1) {}
        }

        todo!()
    }

    /// NOTE(alex): This is less of a system, and more just a function that the user will call,
    /// part of the public API (exposed via `NetManager` client / server).
    pub fn retrieve(&mut self) -> Vec<(ConnectionId, Vec<u8>)> {
        debug!("Retrieving packets.");
        Vec::with_capacity(4)
    }
}
