use std::{
    array::TryFromSliceError,
    net::SocketAddr,
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

    #[error("Tried to schedule a data transfer, but there are no peers connected!")]
    NoPeersConnected,

    #[error("Tried connecting to a Peer that is already in another state `{0}`!")]
    AlreadyConnectingToPeer(SocketAddr),
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
    ConnectionRequest {
        scheduled: Scheduled<Reliable, ConnectionRequest>,
    },
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

impl From<Scheduled<Reliable, DataTransfer>> for ScheduleEvent {
    fn from(scheduled: Scheduled<Reliable, DataTransfer>) -> Self {
        ScheduleEvent::ReliableDataTransfer { scheduled }
    }
}

impl From<Scheduled<Reliable, Fragment>> for ScheduleEvent {
    fn from(scheduled: Scheduled<Reliable, Fragment>) -> Self {
        ScheduleEvent::ReliableFragment { scheduled }
    }
}

impl From<Scheduled<Unreliable, DataTransfer>> for ScheduleEvent {
    fn from(scheduled: Scheduled<Unreliable, DataTransfer>) -> Self {
        ScheduleEvent::UnreliableDataTransfer { scheduled }
    }
}

impl From<Scheduled<Unreliable, Fragment>> for ScheduleEvent {
    fn from(scheduled: Scheduled<Unreliable, Fragment>) -> Self {
        ScheduleEvent::UnreliableFragment { scheduled }
    }
}

impl From<Scheduled<Reliable, ConnectionAccepted>> for ScheduleEvent {
    fn from(scheduled: Scheduled<Reliable, ConnectionAccepted>) -> Self {
        ScheduleEvent::ConnectionAccepted { scheduled }
    }
}

impl From<Scheduled<Reliable, ConnectionRequest>> for ScheduleEvent {
    fn from(scheduled: Scheduled<Reliable, ConnectionRequest>) -> Self {
        ScheduleEvent::ConnectionRequest { scheduled }
    }
}

#[derive(Debug)]
pub enum AntarcEvent {
    Fail(ProtocolError),
    DataTransfer {
        connection_id: ConnectionId,
        payload: Payload,
    },
    ConnectionRequest {
        connection_id: ConnectionId,
        remote: SocketAddr,
    },
    ConnectionAccepted {
        connection_id: ConnectionId,
    },
}

/// TODO(alex) [low] 2021-05-23: These separate event types with a common ground is definitely the
/// way to go, but right now they add a bit too much refactoring work, so come back to this once
/// antarc is properly working.
#[derive(Debug, PartialEq, Clone)]
pub enum ServerEvent {}

#[derive(Debug, PartialEq, Clone)]
pub enum ClientEvent {}
