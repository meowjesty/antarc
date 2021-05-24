use std::{net::SocketAddr, sync::Weak, time::Duration};

use crate::{
    host::Address,
    packet::{ConnectionId, Encoded, Packet, Payload, Queued, Received, Sent, StatusCode},
};

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum EventKind {
    ReadyToReceive,
    ReadyToSend,
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
    ReadyToReceive,
    ReadyToSend,
    SendConnectionRequest { address: SocketAddr },
    SendHeartbeat { address: SocketAddr },
    ReceivedConnectionRequest { address: SocketAddr },
    FailedEncodingPacket { queued: Packet<Queued> },
    FailedSendingPacket { queued: Packet<Queued> },
    SentPacket { sent: Packet<Sent> },
    ReceivedPacket { received: Packet<Received> },
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
            CommonEvent::ReadyToReceive => EventKind::ReadyToReceive,
            CommonEvent::ReadyToSend => EventKind::ReadyToSend,
            CommonEvent::FailedEncodingPacket { .. } => EventKind::FailedEncodingPacket,
            CommonEvent::FailedSendingPacket { .. } => EventKind::FailedSendingPacket,
            CommonEvent::SentPacket { .. } => EventKind::SentPacket,
            CommonEvent::ReceivedPacket { .. } => EventKind::ReceivedPacket,
            CommonEvent::SendConnectionRequest { .. } => EventKind::SendConnectionRequest,
            CommonEvent::ReceivedConnectionRequest { .. } => EventKind::ReceivedConnectionRequest,
            CommonEvent::SendHeartbeat { .. } => EventKind::SendHeartbeat,
        }
    }
}

pub(crate) type EventList = Vec<CommonEvent>;
