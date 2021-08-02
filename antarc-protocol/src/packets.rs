use core::{mem::size_of, time::Duration};
use std::{
    net::SocketAddr,
    num::{NonZeroU16, NonZeroU32},
};

use crc32fast::Hasher;

use crate::{events::AntarcEvent, EventSystem, ProtocolId, PROTOCOL_ID_BYTES};

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

pub trait Deliverable {}
pub trait Messager {}

pub trait Encoder {
    fn encode(&self) -> Vec<u8>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Packet<Delivery, Message>
where
    Delivery: Deliverable,
    Message: Messager + Encoder,
{
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
pub struct ToSend {
    pub meta: MetaDelivery,
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

impl Deliverable for ToSend {}
impl Deliverable for Sent {}
impl Deliverable for Received {}
impl Deliverable for Acked {}

// REGION(alex): Packet `Message` types:
#[derive(Debug, Clone, PartialEq)]
pub struct MetaMessage {
    /// TODO(alex) [low] 2021-08-02: This is a common field for every `Packet`, but it's also being
    /// used by the `Scheduled` types, thus if I take it out from the `MetaMessage`, it has to be
    /// inserted in both structs.
    pub packet_type: PacketType,
}

// TODO(alex) [vlow] 2021-07-30: Add `impl` with associated consts for
// `Packet<Delivery, ConnectionRequest>`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionRequest {
    pub meta: MetaMessage,
}

// TODO(alex) [vlow] 2021-07-30: Add `impl` with associated consts for
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

impl Messager for ConnectionRequest {}
impl Messager for ConnectionAccepted {}
impl Messager for DataTransfer {}
impl Messager for Fragment {}
impl Messager for Heartbeat {}

impl Encoder for ConnectionRequest {
    fn encode(&self) -> Vec<u8> {
        let packet_type_bytes = self.meta.packet_type.to_be_bytes().to_vec();

        packet_type_bytes
    }
}

impl Encoder for ConnectionAccepted {
    fn encode(&self) -> Vec<u8> {
        let packet_type_bytes = self.meta.packet_type.to_be_bytes().to_vec();
        let connection_id_bytes = self.connection_id.get().to_be_bytes().to_vec();

        let encoded = vec![packet_type_bytes, connection_id_bytes].concat();
        encoded
    }
}

impl Encoder for DataTransfer {
    fn encode(&self) -> Vec<u8> {
        let packet_type_bytes = self.meta.packet_type.to_be_bytes().to_vec();

        packet_type_bytes
    }
}

impl Encoder for Fragment {
    fn encode(&self) -> Vec<u8> {
        let packet_type_bytes = self.meta.packet_type.to_be_bytes().to_vec();

        packet_type_bytes
    }
}
impl Encoder for Heartbeat {
    fn encode(&self) -> Vec<u8> {
        let packet_type_bytes = self.meta.packet_type.to_be_bytes().to_vec();

        packet_type_bytes
    }
}

impl<Message> Packet<ToSend, Message>
where
    Message: Messager + Encoder,
{
    pub fn sent(self, time: Duration) -> Packet<Sent, Message> {
        let packet = Packet {
            id: self.id,
            delivery: Sent {
                meta: MetaDelivery {
                    time,
                    remote: self.delivery.meta.remote,
                },
            },
            sequence: self.sequence,
            ack: self.ack,
            message: self.message,
        };

        packet
    }

    pub fn as_raw(&self) -> RawPacket {
        let encoded_message = self.message.encode();

        let sequence_bytes = self.sequence.get().to_be_bytes().to_vec();
        let ack_bytes = self.ack.to_be_bytes().to_vec();

        let mut hasher = Hasher::new();
        let mut bytes = vec![
            PROTOCOL_ID_BYTES.to_vec(),
            sequence_bytes,
            ack_bytes,
            encoded_message,
        ]
        .concat();

        hasher.update(&bytes);
        let crc32 = hasher.finalize();
        debug_assert!(crc32 != 0);

        bytes.append(&mut crc32.to_be_bytes().to_vec());

        let packet_bytes = bytes[size_of::<ProtocolId>()..].to_vec();
        let raw_packet = RawPacket {
            address: self.delivery.meta.remote,
            bytes: packet_bytes,
        };

        raw_packet
    }
}

// TODO(alex) [low] 2021-07-31: Instead of having one master type with optional fields + bools, we
// could have a family of `Scheduled`, much like `Packet`, and the `scheduler_pipe` would take an
// enum of such types.
#[derive(Debug, Clone, PartialEq)]
pub struct Scheduled<Reliability, Message: Messager> {
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
    pub fn connection_request(packet_id: PacketId, address: SocketAddr, time: Duration) -> Self {
        let reliability = Reliable {};
        let meta = MetaMessage {
            packet_type: CONNECTION_REQUEST,
        };
        let message = ConnectionRequest { meta };

        Self {
            packet_id,
            address,
            time,
            reliability,
            message,
        }
    }

    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, ConnectionRequest> {
        let delivery = ToSend {
            meta: MetaDelivery {
                time,
                remote: self.address,
            },
        };
        let packet = Packet {
            id: self.packet_id,
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }
}

impl Scheduled<Reliable, ConnectionAccepted> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, ConnectionAccepted> {
        let delivery = ToSend {
            meta: MetaDelivery {
                time,
                remote: self.address,
            },
        };
        let packet = Packet {
            id: self.packet_id,
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }
}
