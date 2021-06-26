use crate::packets::ConnectionId;

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct ConnectionAccepted {
    connection_id: ConnectionId,
}
