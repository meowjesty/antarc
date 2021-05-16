use std::{
    convert::TryInto,
    net::SocketAddr,
    time::{Duration, Instant},
};

use hecs::Entity;
use log::error;

use crate::packet::{ConnectionId, Packet, Queued, Received, Sent};

pub(crate) mod requesting_connection;
pub(crate) mod sending_connection_request;

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
pub(crate) struct StateEnteredTime(pub(crate) Duration);

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
    /// TODO(alex) 2021-02-13: Do not flood the network, find a way to check if the `rtt` is
    /// increasing due to us flooding the network with packets.
    pub(crate) rtt: Duration,
}

#[derive(Debug, PartialEq)]
pub(crate) struct LatestReceived {
    pub(crate) packet_id: Entity,
}

#[derive(Debug, PartialEq, PartialOrd)]
pub(crate) struct LatestSent {
    pub(crate) packet_id: Entity,
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub(crate) struct Address(pub(crate) SocketAddr);

pub(crate) const RESEND_TIMEOUT_THRESHOLD: Duration = Duration::from_millis(500);
pub(crate) const CONNECTION_TIMEOUT_THRESHOLD: Duration = Duration::new(2, 0);
