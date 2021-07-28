use std::{convert::TryInto, net::SocketAddr, time::Duration};

use log::debug;

use crate::{
    controls::{data_transfer::DataTransfer, heartbeat::Heartbeat},
    packets::{received::Received, sent::Sent, Ack, ConnectionId, Footer, Packet},
    sequence::Sequence,
    PacketId,
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
pub struct Disconnected;

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct RequestingConnection {
    pub attempts: u32,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct AwaitingConnectionAck {
    pub attempts: u32,
}

#[derive(Debug, Clone)]
pub struct Connected {
    pub connection_id: ConnectionId,
    /// TODO(alex) [low] 2021-02-13: Do not flood the network, find a way to check if the `rtt` is
    /// increasing due to us flooding the network with packets.
    pub rtt: Duration,
    pub latest_sent_id: PacketId,
    pub latest_sent_time: Duration,
}

#[derive(Debug, Clone)]
pub struct Host<State> {
    pub sequence_tracker: Sequence,
    pub remote_ack_tracker: Ack,
    pub local_ack_tracker: Ack,
    pub address: SocketAddr,
    pub state: State,
}

impl Host<RequestingConnection> {
    pub fn new(address: SocketAddr) -> Self {
        Self {
            sequence_tracker: Sequence::one(),
            remote_ack_tracker: 1,
            local_ack_tracker: 0,
            address,
            state: RequestingConnection { attempts: 0 },
        }
    }
}

impl Host<AwaitingConnectionAck> {
    pub fn connected(self, connection_id: ConnectionId) -> Host<Connected> {
        Host {
            sequence_tracker: self.sequence_tracker,
            remote_ack_tracker: self.remote_ack_tracker,
            local_ack_tracker: self.local_ack_tracker,
            address: self.address,
            state: Connected {
                connection_id,
                rtt: Duration::default(),
                latest_sent_id: 0,
                latest_sent_time: Duration::default(),
            },
        }
    }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub struct Address(pub SocketAddr);

pub const RESEND_TIMEOUT_THRESHOLD: Duration = Duration::from_millis(500);
pub const CONNECTION_TIMEOUT_THRESHOLD: Duration = Duration::new(2, 0);
