use std::time::Duration;

use hecs::Entity;

use crate::{
    host::Address,
    packet::{Payload, StatusCode},
};

#[derive(Debug, PartialEq)]
pub(crate) struct PreparePacketToSendEvent {
    pub(crate) payload: Payload,
    pub(crate) status_code: StatusCode,
    pub(crate) address: Address,
    pub(crate) destination_id: Entity,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) struct SentPacketEvent {
    pub(crate) destination_id: Entity,
    pub(crate) packet_id: Entity,
    pub(crate) raw_packet_id: Entity,
    pub(crate) time: Duration,
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

#[derive(Debug, PartialEq)]
pub(crate) struct SendPacketEvent {
    pub(crate) raw_packet_id: Entity,
    pub(crate) status_code: StatusCode,
}

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
