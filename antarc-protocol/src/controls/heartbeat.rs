use crate::packets::ConnectionId;

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct Heartbeat {
    connection_id: ConnectionId
}
