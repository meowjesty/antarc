use std::{collections::HashMap, net::SocketAddr};

use crate::{
    hosts::{AwaitingConnectionAck, Connected, Host, RequestingConnection},
    packets::ConnectionId,
    PacketId,
};

#[derive(Debug)]
pub struct ConnectionSystem {
    pub packet_id_tracker: PacketId,
    // TODO(alex) [low] 2021-05-25: Why would I need this kind of host?
    // disconnected: Vec<Host<Disconnected>>,
    // TODO(alex) [mid] 2021-06-08: A `HashMap<SocketAddr, Host<State>>` is probably more
    // appropriate, as this find address is pertinent.
    pub requesting_connection: Vec<Host<RequestingConnection>>,
    pub awaiting_connection_ack: HashMap<ConnectionId, Host<AwaitingConnectionAck>>,
    pub connected: HashMap<ConnectionId, Host<Connected>>,
}

impl ConnectionSystem {
    pub fn new() -> Self {
        Self {
            packet_id_tracker: 0,
            requesting_connection: Vec::with_capacity(32),
            awaiting_connection_ack: HashMap::with_capacity(32),
            connected: HashMap::with_capacity(32),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.requesting_connection.is_empty()
            || self.awaiting_connection_ack.is_empty()
            || self.connected.is_empty()
    }
}
