use core::fmt;
use std::{collections::HashMap, net::SocketAddr, time::Instant};

use mio::{
    net::{TcpListener, TcpSocket, TcpStream, UdpSocket},
    Events, Interest, Poll, Token,
};

use self::server::PacketId;
use crate::{
    connection::Connection,
    events::{FailureEvent, ReceiverEvent, SenderEvent},
    host::{AwaitingConnectionAck, Connected, Host, RequestingConnection},
    packet::{
        payload::{self, Payload},
        queued::Queued,
        received::Received,
        ConnectionId, Packet, Sent,
    },
    MTU_LENGTH,
};

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

pub struct NetManager<ClientOrServer> {
    pub(crate) timer: Instant,
    /// TODO(alex) 2021-02-26: Each `Host` will probably have it's own `buffer`, like the `timer.
    pub(crate) buffer: Vec<u8>,
    pub(crate) net_type: ClientOrServer,
    pub(crate) network: NetworkResource,
    pub(crate) connection: Connection,
    pub(crate) retrievable: Vec<(ConnectionId, Vec<u8>)>,
    pub(crate) retrievable_count: usize,
    pub(crate) event_system: EventSystem,
}

#[derive(Debug)]
pub(crate) struct NetworkResource {
    pub(crate) udp_socket: UdpSocket,
    pub(crate) writable: bool,
    pub(crate) readable: bool,
    pub(crate) poll: Poll,
    pub(crate) events: Events,
}

#[derive(Debug)]
pub(crate) struct EventSystem {
    pub(crate) sender: Vec<SenderEvent>,
    pub(crate) receiver: Vec<ReceiverEvent>,
    pub(crate) failures: Vec<FailureEvent>,
}

impl EventSystem {
    pub(crate) fn new() -> Self {
        let sender = Vec::with_capacity(1024);
        let receiver = Vec::with_capacity(1024);
        let failures = Vec::with_capacity(1024);

        Self {
            sender,
            receiver,
            failures,
        }
    }
}

impl NetworkResource {
    pub(crate) const TOKEN: Token = Token(0xdad);

    pub(crate) fn new(address: &SocketAddr) -> Self {
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
    pub(crate) fn new(address: &SocketAddr, kind: ClientOrServer) -> Self {
        let timer = Instant::now();
        let buffer = vec![0x0; MTU_LENGTH];

        let retrievable = Vec::with_capacity(128);
        let network = NetworkResource::new(address);
        let connection = Connection::new();
        let retrievable_count = 0;

        let event_system = EventSystem::new();

        Self {
            timer,
            buffer,
            net_type: kind,
            network,
            connection,
            retrievable_count,
            event_system,
            retrievable,
        }
    }

    pub fn cancel_packet(&mut self, packet_id: PacketId) -> bool {
        let cancelled_packet = self
            .event_system
            .sender
            .drain_filter(|event| match event {
                SenderEvent::QueuedDataTransfer { packet, .. } => packet.id == packet_id,
                _ => false,
            })
            .next()
            .is_some();

        cancelled_packet
    }
}

impl<T: fmt::Debug> fmt::Debug for NetManager<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetManager")
            .field("timer", &self.timer)
            .field("buffer", &self.buffer.len())
            .field("events", &self.event_system)
            .field("client_or_server", &self.net_type)
            .field("connection", &self.connection)
            .finish()
    }
}
