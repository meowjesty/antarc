use std::{
    array::TryFromSliceError,
    net::SocketAddr,
    num::{NonZeroU32, TryFromIntError},
};

use antarc_macro::FromScheduled;
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

    #[error("Tried to decode a packet with invalid type code of {0}!")]
    InvalidPacketType(PacketType),

    #[error("Tried to schedule a packet for Peer {0} which is not connected!")]
    ScheduledNotConnected(ConnectionId),

    #[error("Tried to schedule a packet {0} for an invalid type of Peer!")]
    ScheduledInvalidPeer(PacketId),

    #[error("Could not find a peer with {0}!")]
    NotFound(ConnectionId),

    #[error("Tried to schedule a data transfer, but there are no peers connected!")]
    NoPeersConnected,

    #[error("Peer with address `{0}` is banned!")]
    Banned(SocketAddr),

    #[error("Tried connecting to a Peer that is already in another state `{0}`!")]
    AlreadyConnectingToPeer(SocketAddr),

    #[error("Received connection accepted for a Peer that is in another state `{0}`!")]
    PeerInAnotherState(SocketAddr),
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

#[derive(Debug, Clone, PartialEq)]
pub enum ReliableSentEvent {
    ConnectionRequest {
        packet: Packet<Sent, ConnectionRequest>,
    },
    ConnectionAccepted {
        packet: Packet<Sent, ConnectionAccepted>,
    },
}

impl From<Packet<Sent, ConnectionRequest>> for ReliableSentEvent {
    fn from(packet: Packet<Sent, ConnectionRequest>) -> Self {
        Self::ConnectionRequest { packet }
    }
}

impl From<Packet<Sent, ConnectionAccepted>> for ReliableSentEvent {
    fn from(packet: Packet<Sent, ConnectionAccepted>) -> Self {
        Self::ConnectionAccepted { packet }
    }
}

impl From<Packet<Sent, DataTransfer>> for ReliableSentEvent {
    fn from(_packet: Packet<Sent, DataTransfer>) -> Self {
        todo!()
    }
}

impl From<Packet<Sent, Heartbeat>> for ReliableSentEvent {
    fn from(_packet: Packet<Sent, Heartbeat>) -> Self {
        todo!()
    }
}

/// TODO(alex) [low] 2021-07-31: Write a macro to derive the proper `Into`, as it's basically just
/// the same thing over and over.
#[derive(Debug, Clone, PartialEq, FromScheduled)]
pub enum ScheduleEvent {
    // #[scheduled(Scheduled<Reliable, ConnectionRequest>)]
    ConnectionRequest {
        scheduled: Scheduled<Reliable, ConnectionRequest>,
    },

    // #[scheduled(Scheduled<Reliable, ConnectionAccepted>)]
    ConnectionAccepted {
        scheduled: Scheduled<Reliable, ConnectionAccepted>,
    },

    // #[scheduled(Scheduled<Reliable, DataTransfer>)]
    ReliableDataTransfer {
        scheduled: Scheduled<Reliable, DataTransfer>,
    },

    // #[scheduled(Scheduled<Reliable, Fragment>)]
    ReliableFragment {
        scheduled: Scheduled<Reliable, Fragment>,
    },

    // #[scheduled(Scheduled<Unreliable, DataTransfer>)]
    UnreliableDataTransfer {
        scheduled: Scheduled<Unreliable, DataTransfer>,
    },

    // #[scheduled(Scheduled<Unreliable, Fragment>)]
    UnreliableFragment {
        scheduled: Scheduled<Unreliable, Fragment>,
    },
}

// impl From<Scheduled<Reliable, DataTransfer>> for ScheduleEvent {
//     fn from(scheduled: Scheduled<Reliable, DataTransfer>) -> Self {
//         Self::ReliableDataTransfer { scheduled }
//     }
// }

// impl From<Scheduled<Reliable, Fragment>> for ScheduleEvent {
//     fn from(scheduled: Scheduled<Reliable, Fragment>) -> Self {
//         Self::ReliableFragment { scheduled }
//     }
// }

// impl From<Scheduled<Unreliable, DataTransfer>> for ScheduleEvent {
//     fn from(scheduled: Scheduled<Unreliable, DataTransfer>) -> Self {
//         Self::UnreliableDataTransfer { scheduled }
//     }
// }

// impl From<Scheduled<Unreliable, Fragment>> for ScheduleEvent {
//     fn from(scheduled: Scheduled<Unreliable, Fragment>) -> Self {
//         Self::UnreliableFragment { scheduled }
//     }
// }

// impl From<Scheduled<Reliable, ConnectionAccepted>> for ScheduleEvent {
//     fn from(scheduled: Scheduled<Reliable, ConnectionAccepted>) -> Self {
//         Self::ConnectionAccepted { scheduled }
//     }
// }

// impl From<Scheduled<Reliable, ConnectionRequest>> for ScheduleEvent {
//     fn from(scheduled: Scheduled<Reliable, ConnectionRequest>) -> Self {
//         Self::ConnectionRequest { scheduled }
//     }
// }

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
