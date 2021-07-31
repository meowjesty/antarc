use std::{
    array::TryFromSliceError,
    num::{NonZeroU32, TryFromIntError},
};

use thiserror::Error;

use crate::{packets::*, *};

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

    #[error("Tried to schedule a packet for Peer {0} which is not connected!")]
    ScheduleInvalidPeer(ConnectionId),

    #[error("Could not find a peer with {0}!")]
    NotFound(ConnectionId),
}

impl Into<AntarcEvent> for ProtocolError {
    fn into(self) -> AntarcEvent {
        AntarcEvent::Fail(self)
    }
}

#[derive(Debug)]
pub enum ReceiverEvent {
    ConnectionRequest {
        packet: Packet<Received, ConnectionRequest>,
    },
    ConnectionAccepted {
        packet: Packet<Received, ConnectionAccepted>,
    },
    DataTransfer {
        packet: Packet<Received, DataTransfer>,
    },
    Heartbeat {
        packet: Packet<Received, Heartbeat>,
    },
}

/// TODO(alex) [low] 2021-07-31: Write a macro to derive the proper `Into`, as it's basically just
/// the same thing over and over.
#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleEvent {
    ConnectionAccepted {
        scheduled: Scheduled<Reliable, ConnectionAccepted>,
    },
    ReliableDataTransfer {
        scheduled: Scheduled<Reliable, DataTransfer>,
    },
    ReliableFragment {
        scheduled: Scheduled<Reliable, Fragment>,
    },
    UnreliableDataTransfer {
        scheduled: Scheduled<Unreliable, DataTransfer>,
    },
    UnreliableFragment {
        scheduled: Scheduled<Unreliable, Fragment>,
    },
}

impl Into<ScheduleEvent> for Scheduled<Reliable, DataTransfer> {
    fn into(self) -> ScheduleEvent {
        ScheduleEvent::ReliableDataTransfer { scheduled: self }
    }
}

impl Into<ScheduleEvent> for Scheduled<Reliable, Fragment> {
    fn into(self) -> ScheduleEvent {
        ScheduleEvent::ReliableFragment { scheduled: self }
    }
}

impl Into<ScheduleEvent> for Scheduled<Unreliable, DataTransfer> {
    fn into(self) -> ScheduleEvent {
        ScheduleEvent::UnreliableDataTransfer { scheduled: self }
    }
}

impl Into<ScheduleEvent> for Scheduled<Unreliable, Fragment> {
    fn into(self) -> ScheduleEvent {
        ScheduleEvent::UnreliableFragment { scheduled: self }
    }
}
impl Into<ScheduleEvent> for Scheduled<Reliable, ConnectionAccepted> {
    fn into(self) -> ScheduleEvent {
        ScheduleEvent::ConnectionAccepted { scheduled: self }
    }
}

#[derive(Debug)]
pub enum AntarcEvent {
    Fail(ProtocolError),
    DataTransfer {
        connection_id: ConnectionId,
        payload: Payload,
    },
}

/// TODO(alex) [low] 2021-05-23: These separate event types with a common ground is definitely the
/// way to go, but right now they add a bit too much refactoring work, so come back to this once
/// antarc is properly working.
#[derive(Debug, PartialEq, Clone)]
pub enum ServerEvent {}

#[derive(Debug, PartialEq, Clone)]
pub enum ClientEvent {}
