use crate::{packets::ConnectionId, payload::Payload};

#[derive(Debug, PartialEq, Clone, PartialOrd)]
pub struct DataTransfer {
    payload: Payload,
    connection_id: ConnectionId,
}
