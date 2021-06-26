use log::error;

use super::{
    header::{ConnectionAccepted, ConnectionRequest, DataTransfer, Header, Heartbeat},
    Ack, StatusCode, ACK, CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST,
    DATA_TRANSFER, HEARTBEAT,
};

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum PacketKind {
    ConnectionRequest,
    ConnectionAccepted,
    ConnectionDenied,
    Ack(Ack),
    DataTransfer,
    Heartbeat,
}

impl From<ConnectionRequest> for StatusCode {
    fn from(_: ConnectionRequest) -> Self {
        CONNECTION_REQUEST
    }
}

impl From<ConnectionAccepted> for StatusCode {
    fn from(_: ConnectionAccepted) -> Self {
        CONNECTION_ACCEPTED
    }
}

impl From<DataTransfer> for StatusCode {
    fn from(_: DataTransfer) -> Self {
        DATA_TRANSFER
    }
}

impl From<Heartbeat> for StatusCode {
    fn from(_: Heartbeat) -> Self {
        HEARTBEAT
    }
}
