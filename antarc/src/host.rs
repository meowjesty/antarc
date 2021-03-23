use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use crate::packet::{ConnectionId, Sequence};

/// TODO(alex) 2021-01-29: Think of `Sessions / Channels` when wondering about connections, it helps
/// when trying to figure out how to keep alive a session (connection), how the communication
/// between hosts occur (channels trasnfer packets), and gives more struct names for similar things.

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
    connection_id: ConnectionId,
}

pub(crate) const RESEND_TIMEOUT_THRESHOLD: Duration = Duration::from_millis(500);
pub(crate) const CONNECTION_TIMEOUT_THRESHOLD: Duration = Duration::new(2, 0);

#[derive(Debug)]
pub(crate) struct Host {
    pub(crate) address: SocketAddr,
    /// TODO(alex) 2021-02-26: Should each `Host` have its own `Instant` to keep track of timings?
    /// This is not a hard requirement, but many functions will end up using the `Instant` to
    /// calculate time related things, so it might as well be in the struct.
    pub(crate) timer: Instant,

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
