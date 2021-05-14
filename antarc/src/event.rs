use std::{net::SocketAddr, sync::Weak, time::Duration};

use hecs::Entity;

use crate::{
    host::{Address, Host},
    packet::{ConnectionId, Packet, PacketKind, Payload, Queued, Received, Sent, StatusCode},
};

#[derive(Debug)]
pub(crate) enum Event {
    QueuedEvent {
        queued: Packet,
        destination: SocketAddr,
    },
    SentEvent {
        sent: Packet,
        destination: SocketAddr,
    },
    ReceivedEvent {
        received: Packet,
        source: SocketAddr,
    },

    FailedEvent {
        fail: String,
    },
}

pub(crate) type EventList = Vec<Event>;

// #[derive(Debug, PartialEq)]
// pub(crate) struct QueuedPacketEvent {
//     pub(crate) packet: Weak<Queued>,
// }

#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) struct SentPacketEvent {
    pub(crate) packet_id: Entity,
    pub(crate) status_code: u16,
}

#[derive(Debug, PartialEq)]
pub(crate) struct SentConnectionRequestEvent {
    pub(crate) packet_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct SentConnectionAcceptedEvent {
    pub(crate) packet_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct SentConnectionDeniedEvent {
    pub(crate) packet_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct SentDataTransferEvent {
    pub(crate) packet_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct SentHeartbeatEvent {
    pub(crate) packet_id: Entity,
}

/// Raised when a packet is first received by the `socket`.
#[derive(Debug, PartialEq)]
pub(crate) struct ReceivedNewPacketEvent {
    pub(crate) packet_id: Entity,
}

/// Raised when the packet `status_code` is identified as being a subset of a connection request.
/// TODO(alex) 2021-04-21: These events could hold more information than just the ids of each entity
/// to avoid the need to query for additional data on each event handler.
#[derive(Debug, PartialEq)]
pub(crate) struct ReceivedConnectionRequestEvent {
    pub(crate) packet_id: Entity,
    pub(crate) source_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct ReceivedConnectionDeniedEvent {
    pub(crate) packet_id: Entity,
    pub(crate) source_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct ReceivedConnectionAcceptedEvent {
    pub(crate) packet_id: Entity,
    pub(crate) source_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct ReceivedDataTransferEvent {
    pub(crate) packet_id: Entity,
    pub(crate) source_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct ReceivedHeartbeatEvent {
    pub(crate) packet_id: Entity,
    pub(crate) source_id: Entity,
}

// #[derive(Debug, PartialEq)]
// pub(crate) struct QueuedPacketEvent {
//     pub(crate) queued_packet_id: Entity,
//     pub(crate) status_code: StatusCode,
//     pub(crate) connection_id: Option<ConnectionId>,
// }

/// Event: `Host` has `Sent` packets that require acking (remote acks local).
#[derive(Debug, PartialEq)]
pub(crate) struct AckLocalPacketEvent {
    pub(crate) packet_id: Entity,
    pub(crate) source_id: Entity,
}

/// Event: `Host` has `Received` packets that will be acked on the next `send` (local acks remote).
#[derive(Debug, PartialEq)]
pub(crate) struct AckRemotePacketEvent {
    pub(crate) packet_id: Entity,
    pub(crate) destination_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct EnqueuedPacket {
    pub(crate) packet_id: Entity,
    pub(crate) destination_id: Entity,
}
