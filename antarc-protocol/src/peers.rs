use std::{net::SocketAddr, time::Duration};

use crate::packets::*;

pub const RESEND_TIMEOUT_THRESHOLD: Duration = Duration::from_millis(500);
pub const CONNECTION_TIMEOUT_THRESHOLD: Duration = Duration::new(2, 0);

#[derive(Debug, Clone)]
pub struct Peer<Connection> {
    pub sequence_tracker: Sequence,
    pub remote_ack_tracker: Ack,
    pub local_ack_tracker: Ack,
    pub address: SocketAddr,
    pub connection: Connection,
}

pub enum SendTo {
    Single { connection_id: ConnectionId },
    Multiple { connection_ids: Vec<ConnectionId> },
    Broadcast,
}

// REGION(alex): Peer `Connection` types.
#[derive(Debug, Default, Clone, PartialEq, PartialOrd)]
pub struct MetaConnection {}

#[derive(Debug, Default, Clone, PartialEq, PartialOrd)]
pub struct Disconnected {
    pub meta: MetaConnection,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct RequestingConnection {
    pub meta: MetaConnection,
    pub attempts: u32,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct AwaitingConnectionAck {
    pub meta: MetaConnection,
    pub attempts: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Connected {
    pub connection_id: ConnectionId,
    pub rtt: Duration,
    pub latest_sent_id: PacketId,
    pub latest_sent_time: Duration,
}

impl Peer<AwaitingConnectionAck> {
    pub fn connected(self, connection_id: ConnectionId) -> Peer<Connected> {
        Peer {
            sequence_tracker: self.sequence_tracker,
            remote_ack_tracker: self.remote_ack_tracker,
            local_ack_tracker: self.local_ack_tracker,
            address: self.address,
            connection: Connected {
                connection_id,
                rtt: Duration::default(),
                latest_sent_id: 0,
                latest_sent_time: Duration::default(),
            },
        }
    }
}
