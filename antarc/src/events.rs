use std::{
    array::TryFromSliceError,
    io,
    net::SocketAddr,
    num::{NonZeroU32, TryFromIntError},
    sync::Weak,
    time::Duration,
};

use thiserror::Error;

use crate::{
    host::Address,
    packet::{
        header::{ConnectionRequest, DataTransfer, Header, Heartbeat},
        payload::{self, Payload},
        queued::Queued,
        received::Received,
        sequence::Sequence,
        ConnectionId, Footer, Packet, Sent, StatusCode,
    },
    ProtocolId,
};
#[derive(Debug, Clone, Error)]
pub enum AntarcError {
    #[error("Protocol id got {:#?}, expected {:#?}", got, expected)]
    InvalidProtocolId { got: u32, expected: ProtocolId },

    #[error("crc32 got {:#?}, expected {:#?}", got, expected)]
    InvalidCrc32 { got: u32, expected: NonZeroU32 },

    #[error("{0}")]
    ArrayConversion(#[from] TryFromSliceError),

    #[error("{0}")]
    IntConversion(#[from] TryFromIntError),
}

#[derive(Debug)]
pub(crate) enum FailureEvent {
    SendDataTransfer {
        packet: Packet<Queued>,
        payload: Payload,
    },
    SendConnectionAccepted {
        packet: Packet<Queued>,
    },
    SendConnectionRequest,
    SendHeartbeat {
        address: SocketAddr,
    },
    AntarcError(AntarcError),
    IO(io::Error),
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum SenderEvent {
    QueuedDataTransfer { packet: Packet<Queued> },
    QueuedConnectionAccepted { packet: Packet<Queued> },
    QueuedConnectionRequest { packet: Packet<Queued> },
    QueuedHeartbeat { address: SocketAddr },
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum ReceiverEvent {
    ConnectionRequest {
        packet: Packet<Received<ConnectionRequest>>,
    },
    DataTransfer {
        packet: Packet<Received<DataTransfer>>,
        payload: Payload,
    },
    Heartbeat {
        packet: Packet<Received<Heartbeat>>,
    },
    AckRemote {
        sequence: Sequence,
    },
}

/// TODO(alex) [low] 2021-05-23: These separate event types with a common ground is definitely the
/// way to go, but right now they add a bit too much refactoring work, so come back to this once
/// antarc is properly working.
#[derive(Debug, PartialEq, Clone)]
pub(crate) enum ServerEvent {}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum ClientEvent {}
