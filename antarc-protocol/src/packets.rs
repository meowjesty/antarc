use core::time::Duration;
use std::{
    net::SocketAddr,
    num::{NonZeroU16, NonZeroU32},
};

use crate::EventSystem;

pub type PacketId = u64;
pub type Sequence = NonZeroU32;
pub type ConnectionId = NonZeroU16;
pub type Ack = u32;
pub type PacketType = u8;
pub type Payload = Vec<u8>;

pub const CONNECTION_REQUEST: PacketType = 1;
pub const CONNECTION_ACCEPTED: PacketType = 2;
pub const CONNECTION_DENIED: PacketType = 3;
pub const HEARTBEAT: PacketType = 4;
pub const DATA_TRANSFER_FULL: PacketType = 5;
pub const DATA_TRANSFER_FRAGMENTED: PacketType = 6;

pub const MAX_FRAGMENT_SIZE: usize = 1500;

#[derive(Debug, Clone, PartialEq)]
pub struct RawPacket {
    pub address: SocketAddr,
    pub bytes: Vec<u8>,
}

impl RawPacket {
    pub fn new(address: SocketAddr, bytes: Vec<u8>) -> Self {
        todo!()
    }

    pub fn decode(self, events: &mut EventSystem) {
        todo!()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Packet<Delivery, Message> {
    pub id: PacketId,
    pub delivery: Delivery,
    pub sequence: Sequence,
    pub ack: Ack,
    pub message: Message,
}

// REGION(alex): Packet `Delivery` types:
#[derive(Debug, Clone, PartialEq)]
pub struct MetaDelivery {
    pub time: Duration,
    pub remote: SocketAddr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    pub meta: MetaDelivery,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Received {
    pub meta: MetaDelivery,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Acked {
    pub meta: MetaDelivery,
}

// REGION(alex): Packet `Message` types:
#[derive(Debug, Clone, PartialEq)]
pub struct MetaMessage {
    pub packet_type: PacketType,
}

// TODO(alex) [mid] 2021-07-30: Add `impl` with associated consts for
// `Packet<Delivery, ConnectionRequest>`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionRequest {
    pub meta: MetaMessage,
}

// TODO(alex) [mid] 2021-07-30: Add `impl` with associated consts for
// `Packet<Delivery, ConnectionAccepted>`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionAccepted {
    pub meta: MetaMessage,
    pub connection_id: ConnectionId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DataTransfer {
    pub meta: MetaMessage,
    pub connection_id: ConnectionId,
    pub payload: Payload,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    pub meta: MetaMessage,
    pub connection_id: ConnectionId,
    pub index: u8,
    pub total: u8,
    pub payload: Payload,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Heartbeat {
    pub meta: MetaMessage,
    pub connection_id: ConnectionId,
}

// TODO(alex) [low] 2021-07-31: Instead of having one master type with optional fields + bools, we
// could have a family of `Scheduled`, much like `Packet`, and the `scheduler_pipe` would take an
// enum of such types.
#[derive(Debug, Clone, PartialEq)]
pub struct Scheduled<Reliability, Message> {
    pub packet_id: PacketId,
    pub address: SocketAddr,
    pub time: Duration,
    pub reliability: Reliability,
    pub message: Message,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reliable {}

#[derive(Debug, Clone, PartialEq)]
pub struct Unreliable {}

impl Scheduled<Reliable, ConnectionRequest> {
    pub fn connection_request(remote_address: SocketAddr) -> Self {
        todo!()
    }
}
