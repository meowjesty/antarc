use std::{net::SocketAddr, sync::Weak, time::Duration};

use hecs::Entity;

use crate::{
    host::Address,
    packet::{ConnectionId, Encoded, Packet, Payload, Queued, Received, Sent, StatusCode},
};

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum EventKind {
    ReadyToReceive,
    ReadyToSend,
    QueuedPacket,
    FailedEncodingPacket,
    FailedSendingPacket,
    SentPacket,
    ReceivedPacket,
    SendConnectionRequest,
    ReceivedConnectionRequest,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum Event {
    ReadyToReceive,
    ReadyToSend,
    SendConnectionRequest { address: SocketAddr },
    ReceivedConnectionRequest { address: SocketAddr },
    QueuedPacket { queued: Packet<Queued> },
    FailedEncodingPacket { queued: Packet<Queued> },
    FailedSendingPacket { encoded: Packet<Encoded> },
    SentPacket { sent: Packet<Sent> },
    ReceivedPacket { received: Packet<Received> },
}

impl Event {
    pub(crate) fn kind(&self) -> EventKind {
        match self {
            Event::ReadyToReceive => EventKind::ReadyToReceive,
            Event::ReadyToSend => EventKind::ReadyToSend,
            Event::QueuedPacket { .. } => EventKind::QueuedPacket,
            Event::FailedEncodingPacket { .. } => EventKind::FailedEncodingPacket,
            Event::FailedSendingPacket { .. } => EventKind::FailedSendingPacket,
            Event::SentPacket { .. } => EventKind::SentPacket,
            Event::ReceivedPacket { .. } => EventKind::ReceivedPacket,
            Event::SendConnectionRequest { .. } => EventKind::SendConnectionRequest,
            Event::ReceivedConnectionRequest { .. } => EventKind::ReceivedConnectionRequest,
        }
    }
}

pub(crate) type EventList = Vec<Event>;
