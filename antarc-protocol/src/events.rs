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

impl<S: ServiceEvent> Into<AntarcEvent<S>> for ProtocolError {
    fn into(self) -> AntarcEvent<S> {
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

#[derive(Debug, Clone, PartialEq, FromSingleVariant)]
pub enum ReliableSentEvent {
    ConnectionRequest {
        packet: Packet<Sent, ConnectionRequest>,
    },
    ConnectionAccepted {
        packet: Packet<Sent, ConnectionAccepted>,
    },
    DataTransfer {
        packet: Packet<Sent, DataTransfer>,
    },
    Heartbeat {
        packet: Packet<Sent, Heartbeat>,
    },
}

#[derive(Debug, Clone, PartialEq, FromSingleVariant)]
pub enum ScheduleConnection {
    ConnectionRequest {
        scheduled: Scheduled<Reliable, ConnectionRequest>,
    },

    ConnectionAccepted {
        scheduled: Scheduled<Reliable, ConnectionAccepted>,
    },
}

// TODO(alex) [high] 2021-08-09: Instead of using an enum and having a schedule pipe, I could take
// the same approach as the connection handling, and have a separate Vec<Scheduled<R, T>> for each
// scheduled type. This would constrain packets to the service type, such as Server not being able
// to schedule a connection request (impossible with the help of the type system).
//
// Such an approach involves a bunch of vectors and maybe some priority mechanism, to avoid being
// stuck sending only scheduled messages of whatever iterator comes first.
//
// Previously, when thinking about issues with this approach, I've thought that maybe we could end
// up stuck in one of these iterators, such as if the server keeps receiving connection requests
// (DDOS), but the one queue fits all has the exact same problems. It would just fill up with a
// bunch of the same messages.
//
// This is possible for `Scheduled`, because it is never dynamic. The API only exposes a
// "data transfer" scheduler, and we know when it should be `DataTransfer` or `Fragment`. Accepting
// and requesting a connection also are known at compile time.
//
// The only true dynamic part of `antarc` comes in the receiver side of things. That's why decode
// requires an enum.
#[derive(Debug, Clone, PartialEq, FromSingleVariant)]
pub enum ScheduleTransfer {
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

pub trait ServiceEvent {}

// TODO(alex) [high] 2021-08-10: I would like to use `S: Servicer`, instead of this `ServiceEvent`
// trait. Not neccessarely the `Servicer` we have right now, but it would be ideal to put every
// `Server` specific implementation under the same umbrella (same for `Client`), instead of creating
// these new (and very similar) traits.
#[derive(Debug)]
pub enum AntarcEvent<S: ServiceEvent> {
    Fail(ProtocolError),
    DataTransfer {
        connection_id: ConnectionId,
        payload: Payload,
    },
    ServiceEvent(S),
}

/// TODO(alex) [low] 2021-05-23: These separate event types with a common ground is definitely the
/// way to go, but right now they add a bit too much refactoring work, so come back to this once
/// antarc is properly working.
#[derive(Debug, PartialEq, Clone)]
pub enum ServerEvent {
    ConnectionRequest {
        connection_id: ConnectionId,
        remote: SocketAddr,
    },
}

#[derive(Debug, PartialEq, Clone)]
pub enum ClientEvent {
    ConnectionAccepted { connection_id: ConnectionId },
}

impl ServiceEvent for ServerEvent {}
impl ServiceEvent for ClientEvent {}
