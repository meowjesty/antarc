use std::{net::SocketAddr, num::NonZeroU32};

use super::{
    header::{Header, HeaderInfo},
    payload::Payload,
    received::Received,
    ConnectionId, Footer,
};
use crate::PacketId;

pub struct PartialPacket {
    pub(crate) id: PacketId,
    pub(crate) header_info: HeaderInfo,
    pub(crate) payload: Payload,
    pub(crate) crc32: NonZeroU32,
    pub(crate) connection_id: Option<ConnectionId>,
    pub(crate) address: SocketAddr,
}
