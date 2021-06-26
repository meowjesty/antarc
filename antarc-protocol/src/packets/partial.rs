use std::{convert::TryFrom, net::SocketAddr, num::NonZeroU32};

use super::{received::Received, ConnectionId, Handshake, Packet, Transfer};
use crate::{events::ProtocolError, header::HeaderInfo, payload::Payload, PacketId};

pub struct PartialPacket {
    pub(crate) id: PacketId,
    pub(crate) header_info: HeaderInfo,
    pub(crate) payload: Payload,
    pub(crate) crc32: NonZeroU32,
    pub(crate) connection_id: Option<ConnectionId>,
    pub(crate) address: SocketAddr,
}

impl TryFrom<PartialPacket> for Packet<Received, Handshake> {
    type Error = ProtocolError;

    fn try_from(partial: PartialPacket) -> Result<Self, Self::Error> {
        todo!()
    }
}

impl TryFrom<PartialPacket> for Packet<Received, Transfer> {
    type Error = ProtocolError;

    fn try_from(partial: PartialPacket) -> Result<Self, Self::Error> {
        todo!()
    }
}
