use std::{
    convert::TryInto,
    net::SocketAddr,
    time::{Duration, Instant},
};

use hecs::Entity;
use log::error;

use crate::packet::{Acked, ConnectionId, Encoded, Packet, Queued, Received, Sent};

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

#[derive(Debug, Clone)]
pub(crate) struct HostInfo {
    pub(crate) address: SocketAddr,
    pub(crate) received: Vec<Packet<Received>>,
    pub(crate) sent: Vec<Packet<Sent>>,
    pub(crate) acked: Vec<Packet<Acked>>,
    pub(crate) queued: Vec<Packet<Queued>>,
    pub(crate) encoded: Vec<Packet<Encoded>>,
}

impl HostInfo {
    pub(crate) fn new(address: SocketAddr) -> Self {
        Self {
            address,
            received: Vec::with_capacity(32),
            sent: Vec::with_capacity(32),
            acked: Vec::with_capacity(32),
            queued: Vec::with_capacity(32),
            encoded: Vec::with_capacity(32),
        }
    }
}

// TODO(alex) 2021-05-17: Should I reverse this with `HostInfo`? We could have a struct `Host` that
// takes an enum `HostState` with these additional fields, this will simplify passing down hosts
// between events and so on.
#[derive(Debug, Clone)]
pub(crate) enum Host {
    Disconnected { info: HostInfo },
    RequestingConnection { info: HostInfo, attempts: u32 },
    AwaitingConnectionResponse { info: HostInfo, time: Duration },
    Connected { info: HostInfo },
}

impl Host {
    pub(crate) fn disconnected(&self) -> bool {
        match  self {
            Host::Disconnected { .. } => true,
            _ => false
        }
    }
}


#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub(crate) struct Address(pub(crate) SocketAddr);

pub(crate) const RESEND_TIMEOUT_THRESHOLD: Duration = Duration::from_millis(500);
pub(crate) const CONNECTION_TIMEOUT_THRESHOLD: Duration = Duration::new(2, 0);
