use std::{collections::HashMap, net::SocketAddr};

use crate::{
    peers::{AwaitingConnectionAck, Connected, Peer, RequestingConnection},
    packets::ConnectionId,
    PacketId,
};

#[derive(Debug)]
pub struct ConnectionSystem {
    pub packet_id_tracker: PacketId,
    pub requesting_connection: HashMap<SocketAddr, Peer<RequestingConnection>>,
    pub awaiting_connection_ack: HashMap<ConnectionId, Peer<AwaitingConnectionAck>>,
    pub connected: HashMap<ConnectionId, Peer<Connected>>,
}

impl Default for ConnectionSystem {
    fn default() -> Self {
        Self::new(32)
    }
}

impl ConnectionSystem {
    pub fn new(expected_hosts_count: usize) -> Self {
        Self {
            packet_id_tracker: 0,
            requesting_connection: HashMap::with_capacity(expected_hosts_count),
            awaiting_connection_ack: HashMap::with_capacity(expected_hosts_count),
            connected: HashMap::with_capacity(expected_hosts_count),
        }
    }

    pub fn already_requesting_connection(&self, address: &SocketAddr) -> bool {
        self.requesting_connection
            .values()
            .any(|host| host.address == *address)
    }

    pub fn already_awaiting_connection_ack(&self, address: &SocketAddr) -> bool {
        self.awaiting_connection_ack
            .values()
            .any(|host| host.address == *address)
    }

    pub fn already_connected(&self, address: &SocketAddr) -> bool {
        self.connected.values().any(|host| host.address == *address)
    }

    pub fn is_empty(&self) -> bool {
        self.requesting_connection.is_empty()
            || self.awaiting_connection_ack.is_empty()
            || self.connected.is_empty()
    }
}
