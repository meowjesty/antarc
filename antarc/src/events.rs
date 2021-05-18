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
    FailedEncodingPacket,
    FailedSendingPacket,
    SentPacket,
    ReceivedPacket,
    SendHeartbeat,
    SendConnectionRequest,
    ReceivedConnectionRequest,
}

#[derive(Debug, PartialEq, Clone)]
pub(crate) enum Event {
    ReadyToReceive,
    ReadyToSend,
    SendConnectionRequest { address: SocketAddr },
    SendHeartbeat { address: SocketAddr },
    ReceivedConnectionRequest { address: SocketAddr },
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
            Event::FailedEncodingPacket { .. } => EventKind::FailedEncodingPacket,
            Event::FailedSendingPacket { .. } => EventKind::FailedSendingPacket,
            Event::SentPacket { .. } => EventKind::SentPacket,
            Event::ReceivedPacket { .. } => EventKind::ReceivedPacket,
            Event::SendConnectionRequest { .. } => EventKind::SendConnectionRequest,
            Event::ReceivedConnectionRequest { .. } => EventKind::ReceivedConnectionRequest,
            Event::SendHeartbeat { .. } => EventKind::SendHeartbeat,
        }
    }
}

pub(crate) type EventList = Vec<Event>;
