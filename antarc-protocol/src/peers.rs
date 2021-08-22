use std::{collections::HashMap, net::SocketAddr, time::Duration};

use crate::packets::*;

pub const RESEND_TIMEOUT_THRESHOLD: Duration = Duration::from_millis(500);
pub const CONNECTION_TIMEOUT_THRESHOLD: Duration = Duration::new(2, 0);

#[derive(Debug, Clone)]
pub struct Peer<Connection> {
    // TODO(alex) #1 [low] 2021-08-22: A very simple way to handle wrapping (and make sequence be an
    // u16) would be to store a pair, such as:
    /// ```
    /// struct Sequence {
    ///     sequence: NonZeroU64,
    ///     wrap_count: NonZeroU64,
    ///     packet_sequence: NonZeroU16,
    /// }
    /// struct Ack {
    ///     ack: NonZeroU64,
    ///     wrap_count: NonZeroU64,
    ///     packet_ack: NonZeroU16,
    /// }
    /// ```
    ///
    /// And then use `packet_sequence * wrap_count` to get the proper sequence value.
    pub sequence_tracker: Sequence,
    pub remote_ack_tracker: Ack,
    pub local_ack_tracker: Ack,
    pub address: SocketAddr,
    pub connection: Connection,
}

#[derive(Debug)]
pub enum SendTo {
    Single { connection_id: ConnectionId },
    Broadcast,
}

// REGION(alex): Peer `Connection` types.
#[derive(Debug, Default, Clone, PartialEq, PartialOrd)]
pub struct MetaConnection {
    pub time_entered_state: Duration,
    pub latest_sent_packet_id: PacketId,
    pub latest_sent_packet_time: Duration,
}

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
    pub meta: MetaConnection,
    pub connection_id: ConnectionId,
    pub rtt: Duration,
    pub reassembler: HashMap<Sequence, Vec<Packet<Received, Fragment>>>,
}

impl Peer<RequestingConnection> {
    pub fn new(time_entered_state: Duration, address: SocketAddr, remote_ack_tracker: u32) -> Self {
        Peer {
            sequence_tracker: Sequence::new(1).unwrap(),
            remote_ack_tracker,
            local_ack_tracker: 0,
            address,
            connection: RequestingConnection {
                meta: MetaConnection {
                    time_entered_state,
                    latest_sent_packet_id: 0,
                    latest_sent_packet_time: Duration::default(),
                },
                attempts: 0,
            },
        }
    }

    pub fn await_connection_ack(self, time_entered_state: Duration) -> Peer<AwaitingConnectionAck> {
        Peer {
            sequence_tracker: self.sequence_tracker,
            remote_ack_tracker: self.remote_ack_tracker,
            local_ack_tracker: self.local_ack_tracker,
            address: self.address,
            connection: AwaitingConnectionAck {
                meta: MetaConnection {
                    time_entered_state,
                    latest_sent_packet_id: self.connection.meta.latest_sent_packet_id,
                    latest_sent_packet_time: self.connection.meta.latest_sent_packet_time,
                },
                attempts: 0,
            },
        }
    }
}

impl Peer<AwaitingConnectionAck> {
    pub fn connected(
        self,
        time_entered_state: Duration,
        connection_id: ConnectionId,
    ) -> Peer<Connected> {
        Peer {
            sequence_tracker: self.sequence_tracker,
            remote_ack_tracker: self.remote_ack_tracker,
            local_ack_tracker: self.local_ack_tracker,
            address: self.address,
            connection: Connected {
                meta: MetaConnection {
                    time_entered_state,
                    latest_sent_packet_id: self.connection.meta.latest_sent_packet_id,
                    latest_sent_packet_time: self.connection.meta.latest_sent_packet_time,
                },
                connection_id,
                rtt: Duration::default(),
                reassembler: HashMap::with_capacity(2),
            },
        }
    }
}

impl Peer<Connected> {
    /// NOTE(alex): Removes expired packets from the reassembler line.
    pub(crate) fn poll(&mut self, now: Duration) {
        let ttl = Duration::from_secs(1);

        let drop_fragment = self
            .connection
            .reassembler
            .values()
            .filter_map(|fragments| {
                let fragment = fragments.last().expect("Fragment must exist!");
                let fragment_time = fragment.delivery.meta.time;

                if fragment_time + ttl > now {
                    Some(fragment.sequence)
                } else {
                    None
                }
            })
            .last();

        if let Some(fragment_id) = drop_fragment {
            self.connection
                .reassembler
                .remove(&fragment_id)
                .expect("Fragment must exist!");
        }
    }
}
