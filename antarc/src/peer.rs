use std::{
    marker::PhantomData,
    net::{SocketAddr, UdpSocket},
    num::{NonZeroU16, NonZeroU32, NonZeroU8},
};

use crate::host::{Connected, Connecting, Disconnected, Host};
use crate::packet::{Acked, Header, Packet, Received, Retrieved, Sent, ToSend};
use crate::CONNECTION_REQUEST;

/// TODO(alex) 2021-02-07: A `Peer<Client>` will connect to the main `Peer<Server>`, and it'll
/// receive information about the other `Peer<Client>` that are connected to the same server. They
/// should probably remain in `disconnected` until there is a server migration (server goes offline
/// for a while), in which case the clients will try to estabilish a new `Peer<Server>` (negotiate
/// using some heuristic to determine which of the known hosts would be best).
///
/// Migration will require having a `into_server` function.
/// Is this the correct abstraction to handle different kinds of peers?
///
/// TODO(alex) 2021-02-15: Can we use the `Header.connection_id` as the `Peer.connected.len()`?
/// The `Client` will always have only 1 item in `Peer.connected`, but the server may have many.
/// Will having a `connection_id: 1`, then disconnecting the peer (after many iterations of the
/// network running), and having a new peer (or the old peer coming back from disconnected) with a
/// `connection_id: 1` cause a conflict? This depends heavily on how we pass the peer identification
/// back to the user, if we rely only on `connection_id` and treat it like this, then the user might
/// think that it's the same peer. This field is a bit more complicated than the `Peer.sequence`, as
/// it kinda has to be unique among many peers, and maybe be the same in case of a
/// disconnect->reconnect peer.
/// ADD(alex) 2021-02-16: Just keep it as part of the Peer, so every connection we check the
/// biggest number, maybe even put a layer above and have a `connection_num` in the network handler.
#[derive(Debug)]
pub struct Peer<Kind> {
    /// TODO(alex): 2021-02-15: `socket` should not be here, I think it belongs in some higher level
    /// manager thingy, as `socket.send` feels weird when used here.
    socket: UdpSocket,
    connection_id: NonZeroU16,
    kind: Kind,
}

#[derive(Debug)]
pub struct Server {
    disconnected: Vec<Host<Disconnected>>,
    connecting: Vec<Host<Connecting>>,
    connected: Vec<Host<Connected>>,
}

#[derive(Debug)]
pub struct Client<State> {
    server_host: Host<State>,
    other_clients: Vec<Host<Disconnected>>,
    _phantom: PhantomData<State>,
}

/// TODO(alex) 2021-02-14: This should be in the `net` crate, I want to avoid having sockets
/// integrated into the lower parts of the protocol. It should handle the state transitions for
/// packets, and connections, but leave the actual send/receive to the `net` crate.
impl Peer<Server> {
    pub fn new_server(address: &SocketAddr) -> Self {
        let socket = UdpSocket::bind(address).unwrap();

        let server = Server {
            disconnected: Vec::with_capacity(8),
            connecting: Vec::with_capacity(8),
            connected: Vec::with_capacity(8),
        };

        Peer {
            socket,
            connection_id: unsafe { NonZeroU16::new_unchecked(1) },
            kind: server,
        }
    }

    pub fn tick(&self) {
        todo!();
    }

    pub fn retrieve(&self) -> Vec<(u32, Vec<u8>)> {
        todo!();
    }

    pub fn enqueue(&self, data: Vec<u8>) {
        todo!()
    }

    pub fn ban_host(&self, host_id: u32) {
        todo!();
    }
}

/// TODO(alex) 2021-02-14: This should be in the `net` crate, I want to avoid having sockets
/// integrated into the lower parts of the protocol. It should handle the state transitions for
/// packets, and connections, but leave the actual send/receive to the `net` crate.
impl Peer<Client<Disconnected>> {
    pub fn new_client(address: &SocketAddr, server_address: SocketAddr) -> Self {
        let socket = UdpSocket::bind(address).unwrap();

        let server_host = Host::new(server_address);

        let client = Client {
            server_host,
            other_clients: Vec::with_capacity(8),
            _phantom: PhantomData::default(),
        };

        Peer {
            socket,
            connection_id: unsafe { NonZeroU16::new_unchecked(1) },
            kind: client,
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
    pub fn connect(self) -> Peer<Client<Connecting>> {
        let server_host = self.kind.server_host.into_connecting(self.connection_id);

        // self.connecting.push(server_host);

        // TODO(alex) 2021-02-17: This is a 2-part problem:
        // 1. Here we need to check if we're wrapping the `u16::MAX` value and getting a zero back,
        //    if so, then we need to clean the `connection_id` sequencer from old values, find an
        //    unused value for it by searching through `disconnected` hosts;
        let connection_id = NonZeroU16::new(self.connection_id.get() + 1)
            .unwrap_or(unsafe { NonZeroU16::new_unchecked(1) });

        let client = Client {
            server_host,
            other_clients: self.kind.other_clients,
            _phantom: PhantomData::default(),
        };

        Peer {
            socket: self.socket,
            connection_id,
            kind: client,
        }
    }

    pub fn tick(&mut self) {}

    pub fn retrieve(&self) -> Vec<(u32, Vec<u8>)> {
        todo!();
    }

    pub fn enqueue(&self, data: Vec<u8>) {
        todo!();
    }
}

impl Peer<Client<Connecting>> {
    pub fn tick(&mut self) {
        let server_host = &mut self.kind.server_host;
        if let Some(to_send) = server_host.send_queue.pop() {
            // socket.send(to_send)
            println!("Sent {:?}", to_send);
            let sent = to_send.sent();

            server_host.sent.push(sent);
            server_host.sequence =
                unsafe { NonZeroU32::new_unchecked(server_host.sequence.get() + 1) };
        }

        if let Some(received) = server_host.received_list.pop() {
            if received.header.ack > 0 {
                if let Some((index, _)) = server_host
                    .sent
                    .iter()
                    .enumerate()
                    .find(|(_, sent)| sent.header.sequence.get() == received.header.ack)
                {
                    let sent = server_host.sent.remove(index);
                    let acked = sent.acked();

                    server_host.acked.push(acked);
                }
            }

            let internal = received.internald();
            server_host.internals.push(internal);
        }
    }

    pub fn connected(self) -> Peer<Client<Connected>> {
        let server_host = self.kind.server_host.into_connected();

        let client = Client {
            server_host,
            other_clients: self.kind.other_clients,
            _phantom: PhantomData::default(),
        };

        Peer {
            socket: self.socket,
            connection_id: self.connection_id,
            kind: client,
        }
    }
}

impl Peer<Client<Connected>> {
    pub fn tick(&mut self) {
        todo!()
    }

    pub fn retrieve(&mut self) -> Vec<(u32, Vec<u8>)> {
        todo!()
    }

    pub fn enqueue(&mut self, data: Vec<u8>) {
        todo!()
    }
}
