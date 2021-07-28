use crate::{packets::ConnectionId, payload::Payload};

#[derive(Debug, PartialEq, Clone, PartialOrd)]
pub struct DataTransfer {
    pub payload: Payload,
    pub connection_id: ConnectionId,
}
