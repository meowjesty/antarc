use std::{net::SocketAddr, sync::Weak, time::Duration};

use crate::{
    host::Address,
    packet::{
        header::Header, ConnectionId, Encoded, Packet, Payload, Queued, Received, Sent, StatusCode,
    },
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
    QueuedDataTransfer { packet: Packet<Queued> },
    QueuedConnectionAccepted { packet: Packet<Queued> },
    QueuedConnectionRequest { packet: Packet<Queued> },
    QueuedHeartbeat { address: SocketAddr },
    ReceivedConnectionRequest { received: Received },
    FailedEncodingPacket { queued: Packet<Queued> },
    FailedSendingPacket { queued: Packet<Queued> },
    SentPacket { sent: Packet<Sent> },
    ReceivedPacket { received: Packet<Received> },
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
            CommonEvent::QueuedDataTransfer { .. } => EventKind::QueuedDataTransfer,
            CommonEvent::QueuedConnectionAccepted { .. } => EventKind::QueuedConnectionAccepted,
            CommonEvent::FailedEncodingPacket { .. } => EventKind::FailedEncodingPacket,
            CommonEvent::FailedSendingPacket { .. } => EventKind::FailedSendingPacket,
            CommonEvent::SentPacket { .. } => EventKind::SentPacket,
            CommonEvent::ReceivedPacket { .. } => EventKind::ReceivedPacket,
            CommonEvent::QueuedConnectionRequest { .. } => EventKind::SendConnectionRequest,
            CommonEvent::ReceivedConnectionRequest { .. } => EventKind::ReceivedConnectionRequest,
            CommonEvent::QueuedHeartbeat { .. } => EventKind::SendHeartbeat,
        }
    }
}

pub(crate) type EventList = Vec<CommonEvent>;
