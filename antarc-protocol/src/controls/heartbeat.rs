use crate::packets::ConnectionId;

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct Heartbeat {
    pub connection_id: ConnectionId
}
