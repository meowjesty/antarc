use std::{convert::TryFrom, net::SocketAddr, num::NonZeroU32};

use super::{
    header::{ConnectionAccepted, ConnectionRequest, DataTransfer, Header, HeaderInfo},
    payload::Payload,
    received::Received,
    ConnectionId, Footer, Packet,
};
use crate::{events::ProtocolError, PacketId};

pub struct PartialPacket {
    pub(crate) id: PacketId,
    pub(crate) header_info: HeaderInfo,
    pub(crate) payload: Payload,
    pub(crate) crc32: NonZeroU32,
    pub(crate) connection_id: Option<ConnectionId>,
    pub(crate) address: SocketAddr,
}

impl TryFrom<PartialPacket> for Packet<Received<ConnectionRequest>> {
    type Error = ProtocolError;

    fn try_from(partial: PartialPacket) -> Result<Self, Self::Error> {
        todo!()
    }
}

impl TryFrom<PartialPacket> for Packet<Received<ConnectionAccepted>> {
    type Error = ProtocolError;

    fn try_from(partial: PartialPacket) -> Result<Self, Self::Error> {
        todo!()
    }
}

impl TryFrom<PartialPacket> for Packet<Received<DataTransfer>> {
    type Error = ProtocolError;

    fn try_from(partial: PartialPacket) -> Result<Self, Self::Error> {
        todo!()
    }
}
