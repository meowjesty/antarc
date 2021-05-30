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
        header::Header, ConnectionId, Encoded, Footer, Packet, Payload, Queued, Received, Sent,
        StatusCode,
    },
    ProtocolId,
};

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum EventKind {
    QueuedDataTransfer,
    QueuedConnectionAccepted,
    FailedEncodingPacket,
    FailedSendingPacket,
    SentPacket,
    ReceivedPacket,
    SendHeartbeat,
    SendConnectionRequest,
    ReceivedConnectionRequest,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum CommonEvent {
    // QueuedDataTransfer { packet: Packet<Queued, DataTransfer> },
    // QueuedConnectionRequest { packet: Packet<Queued, ConnectionRequest> },
    SentPacket { packet: Packet<Sent> },
}

#[derive(Debug, Clone)]
pub enum AntarcError {
    Conversion,
    InvalidProtocolId { got: u32, expected: ProtocolId },
    InvalidCrc32 { got: u32, expected: NonZeroU32 },
    ArrayConversion(TryFromSliceError),
    IntConversion(TryFromIntError),
}

#[derive(Debug)]
pub(crate) enum FailureEvent {
    SendPacket { packet: Packet<Queued> },
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

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum ConnectionEvent {
    ReceivedConnectionRequest { packet: Packet<Received> },
    // ReceivedAccepted { packet: Packet<Received, ConnectionAccepted> },
}

/// TODO(alex) [low] 2021-05-23: These separate event types with a common ground is definitely the
/// way to go, but right now they add a bit too much refactoring work, so come back to this once
/// antarc is properly working.
#[derive(Debug, PartialEq, Clone)]
pub(crate) enum ServerEvent {}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum ClientEvent {
    SendConnectionRequest { address: SocketAddr },
}

impl CommonEvent {
    pub(crate) fn kind(&self) -> EventKind {
        match self {
            CommonEvent::SentPacket { .. } => EventKind::SentPacket,
        }
    }
}

pub(crate) type EventList = Vec<CommonEvent>;
