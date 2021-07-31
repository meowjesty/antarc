use crate::packets::ConnectionId;

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct ConnectionTerminate {
    connection_id: ConnectionId,
}
