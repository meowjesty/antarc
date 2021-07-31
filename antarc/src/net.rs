use core::fmt;
use std::{collections::HashMap, net::SocketAddr, time::Instant};

use antarc_protocol::*;
use mio::{
    net::{TcpListener, TcpSocket, TcpStream, UdpSocket},
    Events, Interest, Poll, Token,
};

use crate::MTU_LENGTH;

pub mod client;
pub mod server;

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

#[derive(Debug, Clone, PartialEq)]
pub enum SendTo {
    All,
    Single(SocketAddr),
}

pub struct NetManager<Service> {
    pub buffer: Vec<u8>,
    pub protocol: Protocol<Service>,
    pub network: NetworkResource,
}

#[derive(Debug)]
pub struct NetworkResource {
    pub udp_socket: UdpSocket,
    pub writable: bool,
    pub readable: bool,
    pub poll: Poll,
    pub events: Events,
}

impl NetworkResource {
    pub const TOKEN: Token = Token(0xdad);

    pub fn new(address: &SocketAddr) -> Self {
        let mut udp_socket = UdpSocket::bind(*address).unwrap();
        let poll = Poll::new().unwrap();
        let events = Events::with_capacity(1024);

        poll.registry()
            .register(
                &mut udp_socket,
                Self::TOKEN,
                Interest::READABLE | Interest::WRITABLE,
            )
            .unwrap();

        Self {
            udp_socket,
            poll,
            events,
            writable: false,
            readable: false,
        }
    }
}

impl<ClientOrServer> NetManager<ClientOrServer> {
    pub fn new(address: &SocketAddr, protocol: Protocol<ClientOrServer>) -> Self {
        let buffer = vec![0x0; MTU_LENGTH];
        let network = NetworkResource::new(address);

        Self {
            buffer,
            protocol,
            network,
        }
    }

    pub fn cancel_packet(&mut self, packet_id: packets::PacketId) -> bool {
        self.protocol.cancel_packet(packet_id)
    }
}

impl<T: fmt::Debug> fmt::Debug for NetManager<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetManager")
            .field("buffer", &self.buffer.len())
            .field("service", &self.protocol.service)
            .finish()
    }
}
