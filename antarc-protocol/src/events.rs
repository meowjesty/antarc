use std::{
    array::TryFromSliceError,
    net::SocketAddr,
    num::{NonZeroU32, TryFromIntError},
};

use thiserror::Error;

use crate::{
    packets::{payload::Payload, scheduled::Scheduled, Packet},
    ProtocolId,
};
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("Protocol id got {:#?}, expected {:#?}", got, expected)]
    InvalidProtocolId { got: u32, expected: ProtocolId },

    #[error("crc32 got {:#?}, expected {:#?}", got, expected)]
    InvalidCrc32 { got: u32, expected: NonZeroU32 },

    #[error("{0}")]
    ArrayConversion(#[from] TryFromSliceError),

    #[error("{0}")]
    IntConversion(#[from] TryFromIntError),
}

#[derive(Debug, PartialEq, Clone)]
pub enum SenderEvent {
    ScheduledDataTransfer {
        packet: Packet<Scheduled>,
        payload: Payload,
    },
    ScheduledConnectionAccepted {
        packet: Packet<Scheduled>,
    },
    ScheduledConnectionRequest {
        packet: Packet<Scheduled>,
    },
    ScheduledHeartbeat {
        address: SocketAddr,
    },
}

/// TODO(alex) [low] 2021-05-23: These separate event types with a common ground is definitely the
/// way to go, but right now they add a bit too much refactoring work, so come back to this once
/// antarc is properly working.
#[derive(Debug, PartialEq, Clone)]
pub enum ServerEvent {}

#[derive(Debug, PartialEq, Clone)]
pub enum ClientEvent {}
