use std::{
    net::{SocketAddr, UdpSocket},
    num::{NonZeroU16, NonZeroU32},
};

use crate::{
    host::{Connected, Connecting, Disconnected, Host},
    peer::NetManager,
};

// #[derive(Debug)]
// struct DisconnectedClient {
//     server: Host<Disconnected>,
//     other_clients: Vec<Host<Disconnected>>,
// }

// #[derive(Debug)]
// struct ConnectingClient {
//     server: Host<Connecting>,
//     other_clients: Vec<Host<Disconnected>>,
// }

// #[derive(Debug)]
// struct ConnectedClient {
//     server: Host<Connected>,
//     other_clients: Vec<Host<Disconnected>>,
// }

#[derive(Debug)]
pub struct Connection<State> {
    server: Host<State>,
    other_clients: Vec<Host<Disconnected>>,
}

#[derive(Debug)]
pub enum Client {
    Disconnected(Connection<Disconnected>),
    Connecting(Connection<Connecting>),
    Connceted(Connection<Connected>),
}

impl Connection<Disconnected> {
    pub(crate) fn into_connecting(self, connection_id: NonZeroU16) -> Connection<Connecting> {
        let server = self.server.into_connecting(connection_id);

        let connecting = Connection {
            server,
            other_clients: self.other_clients,
        };

        connecting
    }
}

impl Connection<Connecting> {
    pub(crate) fn into_connected(self) -> Connection<Connected> {
        let server = self.server.into_connected();

        let connected = Connection {
            server,
            other_clients: self.other_clients,
        };

        connected
    }

    pub(crate) fn tick(&mut self) {
        if let Some(to_send) = self.server.send_queue.pop() {
            // socket.send(to_send)
            println!("Sent {:?}", to_send);
            let sent = to_send.sent();

            self.server.sent.push(sent);
            self.server.sequence =
                unsafe { NonZeroU32::new_unchecked(self.server.sequence.get() + 1) };
        }

        if let Some(received) = self.server.received_list.pop() {
            if received.header.ack > 0 {
                if let Some((index, _)) = self
                    .server
                    .sent
                    .iter()
                    .enumerate()
                    .find(|(_, sent)| sent.header.sequence.get() == received.header.ack)
                {
                    let sent = self.server.sent.remove(index);
                    let acked = sent.acked();

                    self.server.acked.push(acked);
                }
            }

            let internal = received.internald();
            self.server.internals.push(internal);
        }
    }
}

/// TODO(alex) 2021-02-14: This should be in the `net` crate, I want to avoid having sockets
/// integrated into the lower parts of the protocol. It should handle the state transitions for
/// packets, and connections, but leave the actual send/receive to the `net` crate.
impl NetManager<Client> {
    pub fn new_client(address: &SocketAddr, server_address: SocketAddr) -> Self {
        let socket = UdpSocket::bind(address).unwrap();

        let server = Host::new(server_address);

        let client = Connection {
            server,
            other_clients: Vec::with_capacity(8),
        };

        NetManager {
            socket,
            connection_id_tracker: unsafe { NonZeroU16::new_unchecked(1) },
            kind: Client::Disconnected(client),
        }
    }

    /// Creates a new `Host<Connecting>` or updates an existing `Host<Disconnected>`, then adds a
    /// connection request packet to this host's `to_send` list.
    /// TODO(alex) 2021-02-07: Handle the connection request to a server, there are a few points
    /// to consider:
    ///
    /// 1. Is it okay to connect to another client, when this one is already connected to a server?
    ///    - probably not, keep connected checks to empty or single element in the client;
    /// 2. Create connection request packet:
    ///    - must check if there is an outgoing request already, that has not yet been acked.
    ///    - this function should keep itself to packet handling only, do not try to escalate it
    ///    into handling packet loss or anything like that, this will be part of the network
    ///    implementation, as it requires reading incoming packets.
    pub fn connect(&mut self) {
        // FIXME(alex) 2021-02-20: How do we keep the move semantics here, but avoid the borrowing
        // that breaks it? `Connection<Disconnected>::into_connecting` takes `self` to invalidate
        // itself upon returning a `Connection<Connecting>` (I want this), but this doesn't make
        // sense, as we borrow `kind` to `match` it before the conversion is done. Without variants
        // this would simply be a struct being replace by another, so just doing `into_connecting`
        // directly would make sense.
        let connecting = match self.kind {
            Client::Disconnected(disconnected) => {
                disconnected.into_connecting(self.connection_id_tracker)
            }
            fail_state => {
                panic!("fn connect -> Incorrect state {:?}", fail_state)
            }
        };

        // TODO(alex) 2021-02-17: This is a 2-part problem:
        // 1. Here we need to check if we're wrapping the `u16::MAX` value and getting a zero back,
        //    if so, then we need to clean the `connection_id` sequencer from old values, find an
        //    unused value for it by searching through `disconnected` hosts;
        self.connection_id_tracker = NonZeroU16::new(self.connection_id_tracker.get() + 1).unwrap();

        self.kind = Client::Connecting(connecting);
    }

    pub fn connected(&mut self) {
        let connected = match self.kind {
            Client::Connecting(connecting) => connecting.into_connected(),
            fail_state => {
                panic!("fn connected -> Incorrect state {:?}", fail_state)
            }
        };

        self.kind = Client::Connceted(connected);
    }

    pub fn tick(&mut self) {
        match self.kind {
            Client::Disconnected(_) => {}
            Client::Connecting(_) => {}
            Client::Connceted(_) => {}
        }
    }

    pub fn retrieve(&self) -> Vec<(u32, Vec<u8>)> {
        todo!();
    }

    pub fn enqueue(&self, data: Vec<u8>) {
        todo!();
    }
}
