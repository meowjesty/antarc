use std::{convert::TryFrom, net::SocketAddr, num::NonZeroU32};

use super::{received::Received, ConnectionId, Packet};
use crate::{
    controls::{
        connection_accepted::ConnectionAccepted, connection_request::ConnectionRequest,
        data_transfer::DataTransfer, heartbeat::Heartbeat,
    },
    events::ProtocolError,
    header::HeaderInfo,
    payload::Payload,
    PacketId,
};

#[derive(Debug)]
pub struct PartialPacket {
    pub(crate) id: PacketId,
    pub(crate) header_info: HeaderInfo,
    pub(crate) payload: Payload,
    pub(crate) crc32: NonZeroU32,
    pub(crate) connection_id: Option<ConnectionId>,
    pub(crate) address: SocketAddr,
}

impl TryFrom<PartialPacket> for Packet<Received, ConnectionRequest> {
    type Error = ProtocolError;

    fn try_from(partial: PartialPacket) -> Result<Self, Self::Error> {
        todo!()
    }
}

impl TryFrom<PartialPacket> for Packet<Received, ConnectionAccepted> {
    type Error = ProtocolError;

    fn try_from(partial: PartialPacket) -> Result<Self, Self::Error> {
        todo!()
    }
}

impl TryFrom<PartialPacket> for Packet<Received, DataTransfer> {
    type Error = ProtocolError;

    fn try_from(partial: PartialPacket) -> Result<Self, Self::Error> {
        todo!()
    }
}

impl TryFrom<PartialPacket> for Packet<Received, Heartbeat> {
    type Error = ProtocolError;

    fn try_from(partial: PartialPacket) -> Result<Self, Self::Error> {
        todo!()
    }
}

// impl TryFrom<PartialPacket> for Packet<Received, Handshake> {
//     type Error = ProtocolError;

//     fn try_from(partial: PartialPacket) -> Result<Self, Self::Error> {
//         todo!()
//     }
// }

// impl TryFrom<PartialPacket> for Packet<Received, Transfer> {
//     type Error = ProtocolError;

//     fn try_from(partial: PartialPacket) -> Result<Self, Self::Error> {
//         todo!()
//     }
// }
