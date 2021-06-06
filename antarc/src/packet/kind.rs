use log::error;

use super::{
    header::Header, Ack, StatusCode, ACK, CONNECTION_ACCEPTED, CONNECTION_DENIED,
    CONNECTION_REQUEST, DATA_TRANSFER, HEARTBEAT,
};

#[derive(Debug, Clone, PartialEq, Copy)]
pub(crate) enum PacketKind {
    ConnectionRequest,
    ConnectionAccepted,
    ConnectionDenied,
    Ack(Ack),
    DataTransfer,
    Heartbeat,
}

impl PacketKind {
    pub(crate) fn from_header(header: &Header) -> Self {
        match header.status_code {
            CONNECTION_REQUEST => PacketKind::ConnectionRequest,
            CONNECTION_DENIED => PacketKind::ConnectionDenied,
            CONNECTION_ACCEPTED => PacketKind::ConnectionAccepted,
            DATA_TRANSFER => PacketKind::DataTransfer,
            HEARTBEAT => PacketKind::Heartbeat,
            ACK => PacketKind::Ack(header.ack),
            invalid => {
                error!(
                    "Client::on_received_new_packet invalid packet type {:#?}.",
                    invalid
                );
                unreachable!();
            }
        }
    }
}

impl From<PacketKind> for StatusCode {
    fn from(kind: PacketKind) -> Self {
        match kind {
            PacketKind::ConnectionRequest => CONNECTION_REQUEST,
            PacketKind::ConnectionAccepted => CONNECTION_ACCEPTED,
            PacketKind::ConnectionDenied => CONNECTION_DENIED,
            PacketKind::Ack(_) => ACK,
            PacketKind::DataTransfer => DATA_TRANSFER,
            PacketKind::Heartbeat => HEARTBEAT,
        }
    }
}
