use std::{
    array::TryFromSliceError,
    io,
    net::SocketAddr,
    num::{NonZeroU32, TryFromIntError},
    sync::Weak,
    time::Duration,
};

use crate::{
    host::Address,
    packet::{
        header::Header,
        payload::{self, Payload},
        queued::Queued,
        received::Received,
        ConnectionId, Encoded, Footer, Packet, Sent, StatusCode,
    },
    ProtocolId,
};
#[derive(Debug, Clone)]
pub enum AntarcError {
    InvalidProtocolId { got: u32, expected: ProtocolId },
    InvalidCrc32 { got: u32, expected: NonZeroU32 },
    ArrayConversion(TryFromSliceError),
    IntConversion(TryFromIntError),
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
        packet: Packet<Received>,
    },
    DataTransfer {
        packet: Packet<Received>,
        payload: Payload,
    },
    Heartbeat {
        packet: Packet<Received>,
    },
    AckRemote {
        header: Header,
    },
}

/// TODO(alex) [low] 2021-05-23: These separate event types with a common ground is definitely the
/// way to go, but right now they add a bit too much refactoring work, so come back to this once
/// antarc is properly working.
#[derive(Debug, PartialEq, Clone)]
pub(crate) enum ServerEvent {}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum ClientEvent {}
