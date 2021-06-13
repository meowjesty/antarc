use std::net::SocketAddr;

use crate::{
    host::{AwaitingConnectionAck, Connected, Host, RequestingConnection},
    net::server::PacketId,
};

#[derive(Debug)]
pub(crate) struct Connection {
    pub(crate) packet_id_tracker: PacketId,
    // TODO(alex) [low] 2021-05-25: Why would I need this kind of host?
    // disconnected: Vec<Host<Disconnected>>,
    // TODO(alex) [mid] 2021-06-08: A `HashMap<SocketAddr, Host<State>>` is probably more
    // appropriate, as this find address is pertinent.
    pub(crate) requesting_connection: Vec<Host<RequestingConnection>>,
    pub(crate) awaiting_connection_ack: Vec<Host<AwaitingConnectionAck>>,
    pub(crate) connected: Vec<Host<Connected>>,
}

impl Connection {
    pub(crate) fn new() -> Self {
        Self {
            packet_id_tracker: 0,
            requesting_connection: Vec::with_capacity(32),
            awaiting_connection_ack: Vec::with_capacity(32),
            connected: Vec::with_capacity(32),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.requesting_connection.is_empty()
            || self.awaiting_connection_ack.is_empty()
            || self.connected.is_empty()
    }

    pub(crate) fn is_requesting_connection(&self, address: &SocketAddr) -> bool {
        self.requesting_connection
            .iter()
            .any(|h| h.address == *address)
    }

    pub(crate) fn is_awaiting_connection_ack(&self, address: &SocketAddr) -> bool {
        self.awaiting_connection_ack
            .iter()
            .any(|h| h.address == *address)
    }

    pub(crate) fn is_connected(&self, address: &SocketAddr) -> bool {
        self.connected.iter().any(|h| h.address == *address)
    }

    pub(crate) fn is_known(&self, address: &SocketAddr) -> bool {
        self.is_requesting_connection(address)
            || self.is_awaiting_connection_ack(address)
            || self.is_connected(address)
    }
}
