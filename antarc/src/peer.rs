use std::{
    net::{SocketAddr, UdpSocket},
    num::{NonZeroU16, NonZeroU32},
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
#[derive(Debug)]
pub struct Peer<Kind> {
    /// TODO(alex): 2021-02-15: `socket` should not be here, I think it belongs in some higher level
    /// manager thingy, as `socket.send` feels weird when used here.
    socket: UdpSocket,
    disconnected: Vec<Host<Disconnected>>,
    connecting: Vec<Host<Connecting>>,
    connected: Vec<Host<Connected>>,
    kind: Kind,
}

#[derive(Debug)]
pub struct Server;

#[derive(Debug)]
pub struct Client {
    server_address: SocketAddr,
}

/// TODO(alex) 2021-02-14: This should be in the `net` crate, I want to avoid having sockets
/// integrated into the lower parts of the protocol. It should handle the state transitions for
/// packets, and connections, but leave the actual send/receive to the `net` crate.
impl Peer<Server> {
    pub fn new(address: &SocketAddr) -> Self {
        let socket = UdpSocket::bind(address).unwrap();

        Peer {
            socket,
            disconnected: Vec::with_capacity(8),
            connecting: Vec::with_capacity(8),
            connected: Vec::with_capacity(8),
            kind: Server,
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
impl Peer<Client> {
    pub fn new(address: &SocketAddr, server_address: SocketAddr) -> Self {
        let socket = UdpSocket::bind(address).unwrap();

        Peer {
            socket,
            disconnected: Vec::with_capacity(8),
            connecting: Vec::with_capacity(8),
            connected: Vec::with_capacity(8),
            kind: Client { server_address },
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
    /// into handling packet loss or anything like that, this will be part of the network
    /// implementation, as it requires reading incoming packets.
    pub fn connect(&mut self) -> Result<(), String> {
        if !self.connected.is_empty() {
            panic!("Client is already connected {:?}.", self);
        }

        let server = if let Some((index, _)) = self
            .disconnected
            .iter()
            .enumerate()
            .find(|(_, host)| host.state.remote_addr == self.kind.server_address)
        {
            self.disconnected.remove(index)
        } else {
            Host::<Disconnected>::new(self.kind.server_address)
        };

        let mut server = server.into_connecting();

        let connection_header = Header {
            connection_id: unsafe { NonZeroU32::new_unchecked(1) },
            sequence: unsafe { NonZeroU32::new_unchecked(1) },
            ack: 0,
            past_acks: 0,
            kind: unsafe { NonZeroU16::new_unchecked(CONNECTION_REQUEST) },
        };
        let connection_request = Packet::<ToSend>::new(connection_header, vec![0; 10]);
        server.send_queue.push(connection_request);

        self.connecting.push(server);

        Ok(())
    }

    pub fn tick(&mut self) {
        for (index, connecting) in self.connecting.iter_mut().enumerate() {
            if let Some(to_send) = connecting.send_queue.pop() {
                // socket.send(to_send)
                println!("Sent {:?}", to_send);
                let sent = to_send.sent();

                connecting.sent.push(sent);
                connecting.sequence =
                    unsafe { NonZeroU32::new_unchecked(connecting.sequence.get() + 1) };
            }

            if let Some(received) = connecting.received_list.pop() {
                if received.header.ack > 0 {
                    if let Some((index, _)) = connecting
                        .sent
                        .iter()
                        .enumerate()
                        .find(|(_, sent)| sent.header.sequence.get() == received.header.ack)
                    {
                        let sent = connecting.sent.remove(index);
                        let acked = sent.acked();

                        connecting.acked.push(acked);
                    }
                }

                let internal = received.internald();
                connecting.internals.push(internal);
            }
        }

        for (index, connected) in self.connected.iter().enumerate() {}
    }

    pub fn retrieve(&self) -> Vec<(u32, Vec<u8>)> {
        todo!();
    }

    pub fn enqueue(&self, data: Vec<u8>) {
        todo!();
    }
}
