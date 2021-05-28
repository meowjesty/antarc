use std::{
    convert::TryInto,
    net::SocketAddr,
    time::{Duration, Instant},
};

use log::error;

use crate::{
    net::server::PacketId,
    packet::{Ack, Acked, ConnectionId, Encoded, Packet, Queued, Received, Sent, Sequence},
};

/// TODO(alex) 2021-01-29: Think of `Sessions / Channels` when wondering about connections, it helps
/// when trying to figure out how to keep alive a session (connection), how the communication
/// between hosts occur (channels trasnfer packets), and gives more struct names for similar things.

/// TODO(alex) 2021-04-24: There is a difference between many of the client / server host states and
/// how packets are to be handled on each end.
///
/// A client that is in `RequestingConnection` state is someone who sent a connection request packet
/// and will poll and retry sending this same packet until it receives an ack (reliable handshake).
///
/// Meanwhile a server that receives a connection request wants to send back a connection accepted
/// (or denied) packet.
///
/// The `RequestingConnection` seems to make more sense for hosts on the server side, while the
/// client will have a host in the `SendingConnectionRequest` state of sorts.
///
/// This is why writing some client and server APIs right now makes more sense, even if it's just
/// littered with `todo!()`, as I need to get a better understanding on what states belong where.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub(crate) struct Disconnected;

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub(crate) struct RequestingConnection {
    pub(crate) attempts: u32,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub(crate) struct SendingConnectionRequest {
    pub(crate) attempts: u32,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub(crate) struct AwaitingConnectionAck {
    pub(crate) attempts: u32,
    pub(crate) connection_id: ConnectionId,
    pub(crate) last_sent: PacketId,
}

#[derive(Debug, Clone)]
pub(crate) struct AwaitingConnectionResponse {
    pub(crate) attempts: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct AckingConnection {
    attempts: u32,
    pub(crate) connection_id: ConnectionId,
}

#[derive(Debug, Clone)]
pub(crate) struct Connected {
    pub(crate) connection_id: ConnectionId,
    /// TODO(alex) [low] 2021-02-13: Do not flood the network, find a way to check if the `rtt` is
    /// increasing due to us flooding the network with packets.
    pub(crate) rtt: Duration,
    pub(crate) last_sent: PacketId,
    pub(crate) time_last_sent: Duration,
}

#[derive(Debug, Clone)]
pub(crate) struct Host<State> {
    pub(crate) sequence_tracker: Sequence,
    pub(crate) remote_ack_tracker: Ack,
    pub(crate) local_ack_tracker: Ack,
    pub(crate) address: SocketAddr,
    // TODO(alex) [mid] 2021-05-26: `Received` should not contain the payload, let's put the
    // payload in a `HashMap<ConnectionId, Vec<Payload>>` in the server/client, to ease the
    // `retrieve` impl.
    pub(crate) received: Vec<Packet<Received>>,
    pub(crate) state: State,
}

impl Host<Disconnected> {
    pub(crate) fn new_disconnected(address: SocketAddr) -> Self {
        let state = Disconnected;
        Self {
            sequence_tracker: Sequence::default(),
            remote_ack_tracker: 0,
            local_ack_tracker: 0,
            address,
            received: Vec::with_capacity(32),
            state,
        }
    }
}

impl Host<Generic> {
    pub(crate) fn new_generic(address: SocketAddr) -> Self {
        let state = Generic {
            state: HostState::Disconnected,
        };

        Self {
            sequence_tracker: Sequence::default(),
            remote_ack_tracker: 0,
            local_ack_tracker: 0,
            address,
            received: Vec::with_capacity(32),
            state,
        }
    }

    pub(crate) fn new_local() -> Self {
        Self::new_generic("127.0.0.1:7777".parse().unwrap())
    }

    pub(crate) fn disconnected(&self) -> bool {
        match self.state.state {
            HostState::Disconnected => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Generic {
    // TODO(alex) 2021-05-17: This name is horrible, the naming conventions used here are kinda
    // confusing in general, `host.rs` file warrants a big renaming.
    pub(crate) state: HostState,
}

// TODO(alex) 2021-05-17: Should I reverse this with `HostInfo`? We could have a struct `Host` that
// takes an enum `HostState` with these additional fields, this will simplify passing down hosts
// between events and so on.
#[derive(Debug, Clone)]
pub(crate) enum HostState {
    Disconnected,
    RequestingConnection { info: RequestingConnection },
    AwaitingConnectionResponse { info: AwaitingConnectionResponse },
    Connected { info: Connected },
}

impl HostState {
    pub(crate) fn connection_id(&self) -> Option<ConnectionId> {
        match self {
            HostState::Connected { info } => Some(info.connection_id),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub(crate) struct Address(pub(crate) SocketAddr);

pub(crate) const RESEND_TIMEOUT_THRESHOLD: Duration = Duration::from_millis(500);
pub(crate) const CONNECTION_TIMEOUT_THRESHOLD: Duration = Duration::new(2, 0);
