use std::net::SocketAddr;

use crate::packet::{Acked, Packet, Received, Retrieved, Sent, ToSend};

#[derive(Debug)]
struct Connecting {
    remote_addr: SocketAddr,
}

#[derive(Debug)]
struct Connected {
    remote_addr: SocketAddr,
}

#[derive(Debug)]
struct Disconnected {
    remote_addr: SocketAddr,
}

#[derive(Debug)]
struct Host<State> {
    received_list: Vec<Packet<Received>>,
    retrieved: Vec<Packet<Retrieved>>,
    send_queue: Vec<Packet<ToSend>>,
    sent: Vec<Packet<Sent>>,
    acked: Vec<Packet<Acked>>,
    state: State,
}

impl Host<Disconnected> {
    /// TODO(alex) 2021-02-07: Is it possible to create a `Host` in any other state? Or should it
    /// always start in disconnected mode?
    pub(crate) fn new(remote_addr: SocketAddr) -> Host<Disconnected> {
        let state = Disconnected { remote_addr };
        let host = Host {
            received_list: Vec::with_capacity(32),
            retrieved: Vec::with_capacity(32),
            send_queue: Vec::with_capacity(32),
            sent: Vec::with_capacity(32),
            acked: Vec::with_capacity(32),
            state,
        };

        host
    }

    fn into_connecting(self) -> Host<Connecting> {
        let state = Connecting {
            remote_addr: "127.0.0.1:7777".parse().unwrap(),
        };

        let host = Host {
            received_list: self.received_list,
            retrieved: self.retrieved,
            send_queue: self.send_queue,
            sent: self.sent,
            acked: self.acked,
            state,
        };

        host
    }
}

impl Host<Connecting> {
    fn into_connected(self) -> Host<Connected> {
        let state = Connected {
            remote_addr: "127.0.0.1:7777".parse().unwrap(),
        };

        let host = Host {
            received_list: self.received_list,
            retrieved: self.retrieved,
            send_queue: self.send_queue,
            sent: self.sent,
            acked: self.acked,
            state,
        };

        host
    }
}

#[derive(Debug)]
struct Client;
#[derive(Debug)]
struct Server;

/// TODO(alex) 2021-02-07: A `Peer<Client>` will connect to the main `Peer<Server>`, and it'll
/// receive information about the other `Peer<Client>` that are connected to the same server. They
/// should probably remain in `disconnected` until there is a server migration (server goes offline
/// for a while), in which case the clients will try to estabilish a new `Peer<Server>` (negotiate
/// using some heuristic to determine which of the known hosts would be best).
///
/// Migration will require having a `into_server` function.
/// Is this the correct abstraction to handle different kinds of peers?
#[derive(Debug)]
struct Peer<Kind> {
    connecting: Vec<Host<Connecting>>,
    connected: Vec<Host<Connected>>,
    disconnected: Vec<Host<Disconnected>>,
    kind: Kind,
}

impl Peer<Client> {
    fn new(server_address: &SocketAddr) -> Self {
        let server = Host::new(*server_address);
        let mut disconnected = Vec::with_capacity(8);
        disconnected.push(server);

        Self {
            connecting: Vec::with_capacity(8),
            connected: Vec::with_capacity(8),
            disconnected,
            kind: Client,
        }
    }

    /// TODO(alex) 2021-02-07: Handle the connection request to a server, there are a few points
    /// to consider:
    ///
    /// 1. Is it okay to connect to another client, when this one is already connected to a server?
    ///    - probably not, keep connected checks to empty or single element in the client;
    /// 2. Create connection request packet:
    ///    - must check if there is an outgoing request already, that has not yet been acked.
    ///    - this function should keep itself to packet handling only, do not try to escalate it
    /// into handling packet loss or anything like that, this will be part of the network
    /// implementation, as it requires reading incoming packets.
    fn connect(&self) {
        if !self.connected.is_empty() {
            panic!("Client is already connected {:?}.", self);
        }
    }
}

impl Peer<Server> {
    fn new() -> Self {
        Self {
            connecting: Vec::with_capacity(8),
            connected: Vec::with_capacity(8),
            disconnected: Vec::with_capacity(8),
            kind: Server,
        }
    }
}
