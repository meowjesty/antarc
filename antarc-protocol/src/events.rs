use std::{
    array::TryFromSliceError,
    net::SocketAddr,
    num::{NonZeroU32, TryFromIntError},
};

use antarc_macro::FromSingleVariant;
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

    #[error("`from_be_bytes` failed to convert array into valid `NonZero` value!")]
    BytesConversion,

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

#[derive(Debug, Clone, PartialEq, FromSingleVariant)]
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
