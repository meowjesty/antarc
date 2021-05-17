use core::fmt;
use std::{net::SocketAddr, time::Instant};

use hecs::{Entity, World};
use mio::{
    net::{TcpListener, TcpSocket, TcpStream, UdpSocket},
    Events, Interest, Poll, Token,
};

use crate::{
    event::EventList,
    packet::{Packet, Queued, Received, Sent},
    MTU_LENGTH,
};

pub mod client;
pub mod server;

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum ConnectionState {
    RequestingConnection,
    AwaitingConnectionResponse,
    Disconnected,
    Connected,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct Connection {
    pub(crate) queued: Vec<Packet<Queued>>,
    pub(crate) received: Vec<Packet<Received>>,
    pub(crate) state: ConnectionState,
}

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

pub struct NetManager<ClientOrServer> {
    pub(crate) timer: Instant,
    /// TODO(alex) 2021-02-26: Each `Host` will probably have it's own `buffer`, like the `timer.
    pub(crate) buffer: Vec<u8>,
    pub(crate) client_or_server: ClientOrServer,
    pub(crate) network: NetworkResource,
    pub(crate) events: EventList,
}

#[derive(Debug)]
pub(crate) struct NetworkResource {
    pub(crate) tcp_stream: TcpStream,
    pub(crate) udp_socket: UdpSocket,
    pub(crate) poll: Poll,
    pub(crate) events: Events,
}

impl NetworkResource {
    pub(crate) const TOKEN: Token = Token(0xdad);

    pub(crate) fn new(address: &SocketAddr) -> Self {
        // TODO(alex) 2021-05-11: Change this to `TcpSocket`, but then we cannot register with the
        // poller here (tcp), the `Client::connect` will do the actual `bind` call and register.
        let mut tcp_stream = TcpStream::connect(*address).unwrap();
        let mut udp_socket = UdpSocket::bind(*address).unwrap();
        let poll = Poll::new().unwrap();
        let events = Events::with_capacity(1024);

        poll.registry()
            .register(
                &mut tcp_stream,
                Self::TOKEN,
                Interest::READABLE | Interest::WRITABLE,
            )
            .unwrap();

        poll.registry()
            .register(
                &mut udp_socket,
                Self::TOKEN,
                Interest::READABLE | Interest::WRITABLE,
            )
            .unwrap();

        Self {
            tcp_stream,
            udp_socket,
            poll,
            events,
        }
    }
}

impl<ClientOrServer> NetManager<ClientOrServer> {
    pub(crate) fn new(address: &SocketAddr, client_or_server: ClientOrServer) -> Self {
        let timer = Instant::now();
        let buffer = vec![0x0; MTU_LENGTH];
        let events = Vec::with_capacity(1024);

        let network_resource = NetworkResource::new(address);

        Self {
            timer,
            events,
            buffer,
            client_or_server,
            network: network_resource,
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for NetManager<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetManager")
            .field("timer", &self.timer)
            .field("buffer", &self.buffer.len())
            .field("events", &self.events.len())
            .field("client_or_server", &self.client_or_server)
            .finish()
    }
}
