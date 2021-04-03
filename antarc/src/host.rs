use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use crate::packet::{ConnectionId, Sequence};

pub(crate) mod requesting_connection;
pub(crate) mod sending_connection_request;

/// TODO(alex) 2021-01-29: Think of `Sessions / Channels` when wondering about connections, it helps
/// when trying to figure out how to keep alive a session (connection), how the communication
/// between hosts occur (channels trasnfer packets), and gives more struct names for similar things.

#[derive(Debug)]
pub(crate) struct Disconnected;

#[derive(Debug)]
pub(crate) struct RequestingConnection {
    pub(crate) attempts: u32,
}

#[derive(Debug)]
pub(crate) struct SendingConnectionRequest {
    attempts: u32,
}

#[derive(Debug)]
pub(crate) struct AwaitingConnectionAck {
    attempts: u32,
}

#[derive(Debug)]
pub(crate) struct Connected {
    pub(crate) connection_id: ConnectionId,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) struct Address(pub(crate) SocketAddr);

pub(crate) const RESEND_TIMEOUT_THRESHOLD: Duration = Duration::from_millis(500);
pub(crate) const CONNECTION_TIMEOUT_THRESHOLD: Duration = Duration::new(2, 0);

#[derive(Debug, Clone)]
pub(crate) struct Host {
    /// NOTE(alex) 2021-03-02: `sequence` is incremented only after a packet is successfully sent
    /// (`Packet<Sent>`), this is done to prevent remote `Host`s from thinking that some packets
    /// were lost, even in the case of them never being sent.
    pub(crate) sequence_tracker: Sequence,
    pub(crate) ack_tracker: u32,
    pub(crate) past_acks_tracker: u16,
    /// TODO(alex) 2021-02-13: Do not flood the network, find a way to check if the `rtt` is
    /// increasing due to us flooding the network with packets.
    pub(crate) rtt: Duration,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            sequence_tracker: unsafe { Sequence::new_unchecked(1) },
            ack_tracker: 0,
            past_acks_tracker: 0,
            rtt: Duration::default(),
        }
    }
}
