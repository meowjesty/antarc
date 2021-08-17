use std::{
    array::TryFromSliceError,
    net::SocketAddr,
    num::{NonZeroU32, TryFromIntError},
};

use thiserror::Error;

use crate::{packets::*, *};

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Protocol id got {:#?}, expected {:#?}", got, expected)]
    InvalidProtocolId { got: u32, expected: ProtocolId },

    #[error("crc32 got {:#?}, expected {:#?}", got, expected)]
    InvalidCrc32 { got: u32, expected: NonZeroU32 },

    #[error("{0}")]
    ArrayConversion(#[from] TryFromSliceError),

    #[error("`from_be_bytes` failed to convert array into valid `NonZero` value!")]
    BytesConversion,

    #[error("{0}")]
    IntConversion(#[from] TryFromIntError),

    #[error("Tried to decode a packet with invalid type code of {0}!")]
    InvalidPacketType(PacketType),

    #[error("Tried to schedule a packet for Peer {0} which is not connected!")]
    ScheduledNotConnected(ConnectionId),

    #[error("Tried to schedule a packet {0} for an invalid type of Peer!")]
    ScheduledInvalidPeer(PacketId),

    #[error("Could not find a peer with {0}!")]
    NotFound(ConnectionId),

    #[error("Tried to schedule a data transfer, but there are no peers connected!")]
    NoPeersConnected,

    #[error("Peer with address `{0}` is banned!")]
    Banned(SocketAddr),

    #[error("Tried connecting to a Peer that is already in another state `{0}`!")]
    AlreadyConnectingToPeer(SocketAddr),

    #[error("Received connection accepted for a Peer that is in another state `{0}`!")]
    PeerInAnotherState(SocketAddr),
}
