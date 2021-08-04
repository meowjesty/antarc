use core::{mem::size_of, time::Duration};
use std::{
    convert::TryInto,
    net::SocketAddr,
    num::{NonZeroU16, NonZeroU32},
};

use crc32fast::Hasher;
use log::{debug, error};

use crate::{
    events::{AntarcEvent, ProtocolError, ReceiverEvent},
    read_buffer_inc, EventSystem, ProtocolId, PROTOCOL_ID, PROTOCOL_ID_BYTES,
};

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
        Self { address, bytes }
    }

    pub fn decode(self, events: &mut EventSystem, id: PacketId, time: Duration) {
        let mut hasher = Hasher::new();

        let length = self.bytes.len();
        let bytes = self.bytes;

        let crc32_position = length - size_of::<ProtocolId>();
        let crc32_bytes: &[u8; size_of::<NonZeroU32>()] =
            bytes[crc32_position..].try_into().unwrap();
        let crc32_received = u32::from_be_bytes(*crc32_bytes);

        // NOTE(alex): Cannot use the full buffer when re-calculating the crc32 for comparison, as
        // the crc32 is calculated after encoding.
        let bytes_without_crc32 = &bytes[..crc32_position];

        let packet_with_protocol_id = [&PROTOCOL_ID_BYTES, bytes_without_crc32].concat();

        hasher.update(&packet_with_protocol_id);
        let crc32 = hasher.finalize();

        if crc32 == crc32_received {
            let buffer = packet_with_protocol_id;
            let mut buffer_position = 0;

            // TODO(alex) [low] 2021-08-04: Find out a way to make `from_be_bytes::<ProtocolId>`
            // work.
            let read_protocol_id = read_buffer_inc!({buffer, buffer_position } : u32);
            if PROTOCOL_ID.get() != read_protocol_id {
                events.api.push(
                    ProtocolError::InvalidProtocolId {
                        got: read_protocol_id,
                        expected: PROTOCOL_ID,
                    }
                    .into(),
                );

                return;
            }

            let read_sequence = read_buffer_inc!({ buffer, buffer_position } : u32);
            debug_assert_ne!(read_sequence, 0);

            let read_ack = read_buffer_inc!({ buffer, buffer_position } : Ack);
            let read_packet_type = read_buffer_inc!({ buffer, buffer_position } : PacketType);
            match read_packet_type {
                ConnectionRequest::PACKET_TYPE => {
                    debug!("Decoding connection request packet.");
                    debug_assert_eq!(buffer_position, ConnectionRequest::HEADER_SIZE);

                    let delivery = Received {
                        meta: MetaDelivery {
                            time,
                            remote: self.address,
                        },
                    };
                    let message = ConnectionRequest {
                        meta: MetaMessage {
                            packet_type: read_packet_type,
                        },
                    };

                    let packet = Packet {
                        id,
                        delivery,
                        sequence: read_sequence.try_into().unwrap(),
                        ack: read_ack,
                        message,
                    };

                    events
                        .receiver
                        .push(ReceiverEvent::ConnectionRequest { packet });
                }
                ConnectionAccepted::PACKET_TYPE => {
                    debug!("Decoding connection accepted packet.");

                    // TODO(alex) [low] 2021-08-04: Find out a way to make
                    // `from_be_bytes::<ConnectionId>` work.
                    let read_connection_id = read_buffer_inc!({ buffer, buffer_position } : u16);
                    debug_assert_eq!(buffer_position, ConnectionAccepted::HEADER_SIZE);

                    let delivery = Received {
                        meta: MetaDelivery {
                            time,
                            remote: self.address,
                        },
                    };
                    let message = ConnectionAccepted {
                        meta: MetaMessage {
                            packet_type: read_packet_type,
                        },
                        connection_id: read_connection_id.try_into().unwrap(),
                    };

                    let packet = Packet {
                        id,
                        delivery,
                        sequence: read_sequence.try_into().unwrap(),
                        ack: read_ack,
                        message,
                    };

                    events
                        .receiver
                        .push(ReceiverEvent::ConnectionAccepted { packet });
                }
                DataTransfer::PACKET_TYPE => {
                    debug!("Decoding data transfer packet.");

                    // TODO(alex) [low] 2021-08-04: Find out a way to make
                    // `from_be_bytes::<ConnectionId>` work.
                    let read_connection_id = read_buffer_inc!({ buffer, buffer_position } : u16);
                    debug_assert_eq!(buffer_position, DataTransfer::HEADER_SIZE);

                    let read_payload = buffer[buffer_position..].to_vec();

                    let delivery = Received {
                        meta: MetaDelivery {
                            time,
                            remote: self.address,
                        },
                    };
                    let message = ConnectionAccepted {
                        meta: MetaMessage {
                            packet_type: read_packet_type,
                        },
                        connection_id: read_connection_id.try_into().unwrap(),
                    };

                    let packet = Packet {
                        id,
                        delivery,
                        sequence: read_sequence.try_into().unwrap(),
                        ack: read_ack,
                        message,
                    };

                    events
                        .receiver
                        .push(ReceiverEvent::ConnectionAccepted { packet });
                }
                Fragment::PACKET_TYPE => {
                    debug!("Decoding fragment packet.");
                    todo!();
                }
                Heartbeat::PACKET_TYPE => {
                    debug!("Decoding heartbeat packet.");
                    todo!();
                }
                invalid => {
                    error!("Decoding invalid packet type {:#?}.", invalid);
                    events
                        .api
                        .push(ProtocolError::InvalidPacketType(invalid).into());
                    return;
                }
            };
        }
    }
}

pub trait Deliver {}
pub trait Messager {
    const PACKET_TYPE: PacketType;
}

pub trait Encoder {
    const HEADER_SIZE: usize;
    fn encoded(&self) -> Vec<u8>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Packet<Delivery, Message>
where
    Delivery: Deliver,
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

impl Deliver for ToSend {}
impl Deliver for Sent {}
impl Deliver for Received {}
impl Deliver for Acked {}

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

impl Messager for ConnectionRequest {
    const PACKET_TYPE: PacketType = 0x1;
}

impl Messager for ConnectionAccepted {
    const PACKET_TYPE: PacketType = 0x2;
}

impl Messager for DataTransfer {
    const PACKET_TYPE: PacketType = 0x3;
}

impl Messager for Fragment {
    const PACKET_TYPE: PacketType = 0x4;
}
impl Messager for Heartbeat {
    const PACKET_TYPE: PacketType = 0x5;
}

impl Encoder for ConnectionRequest {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = self.meta.packet_type.to_be_bytes().to_vec();

        packet_type_bytes
    }
}

impl Encoder for ConnectionAccepted {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = self.meta.packet_type.to_be_bytes().to_vec();
        let connection_id_bytes = self.connection_id.get().to_be_bytes().to_vec();

        let encoded = vec![packet_type_bytes, connection_id_bytes].concat();
        encoded
    }
}

impl Encoder for DataTransfer {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>()
        + size_of::<u16>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = self.meta.packet_type.to_be_bytes().to_vec();

        packet_type_bytes
    }
}

impl Encoder for Fragment {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>()
        // TODO(alex) [mid] 2021-08-04: Missing fragmend index, fragment total sizes.
        + size_of::<u16>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = self.meta.packet_type.to_be_bytes().to_vec();

        packet_type_bytes
    }
}
impl Encoder for Heartbeat {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>();

    fn encoded(&self) -> Vec<u8> {
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
        let encoded_message = self.message.encoded();

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
