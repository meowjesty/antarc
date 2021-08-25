use core::{mem::size_of, ops::Range, time::Duration};
use std::{
    convert::{TryFrom, TryInto},
    marker::PhantomData,
    net::SocketAddr,
    num::{NonZeroU16, NonZeroU32},
    sync::Arc,
};

use crc32fast::Hasher;
use log::*;

use self::packet_type::PacketType;
use crate::{client::*, errors::*, server::*, *};

pub mod packet_type;

pub type PacketId = u64;
pub type Sequence = NonZeroU32;
pub type ConnectionId = NonZeroU16;
pub type Ack = u32;
// pub type PacketType = u8;
pub type Payload = Vec<u8>;

pub const MAX_FRAGMENT_SIZE: usize = 1500;

#[derive(Debug, Clone, PartialEq)]
pub struct PartialDecode {
    pub buffer: Vec<u8>,
    pub buffer_position: usize,
    pub packet_type: PacketType,
    pub sequence: Sequence,
    pub ack: Ack,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawPacket {
    pub address: SocketAddr,
    pub bytes: Vec<u8>,
}

pub trait Decoder {}

#[derive(Debug, Clone, PartialEq)]
pub enum DecodedForClient {
    ConnectionAccepted {
        packet: Packet<Received, ConnectionAccepted>,
    },
    DataTransfer {
        packet: Packet<Received, DataTransfer>,
    },
    Fragment {
        packet: Packet<Received, Fragment>,
    },
    Heartbeat {
        packet: Packet<Received, Heartbeat>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecodedForServer {
    ConnectionRequest {
        packet: Packet<Received, ConnectionRequest>,
    },
    DataTransfer {
        packet: Packet<Received, DataTransfer>,
    },
    Fragment {
        packet: Packet<Received, Fragment>,
    },
    Heartbeat {
        packet: Packet<Received, Heartbeat>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecodedCommon {
    ConnectionRequest {
        packet: Packet<Received, ConnectionRequest>,
    },
    ConnectionAccepted {
        packet: Packet<Received, ConnectionAccepted>,
    },
    DataTransfer {
        packet: Packet<Received, DataTransfer>,
    },
    Fragment {
        packet: Packet<Received, Fragment>,
    },
    Heartbeat {
        packet: Packet<Received, Heartbeat>,
    },
}

impl TryFrom<DecodedCommon> for DecodedForServer {
    type Error = ProtocolError;

    fn try_from(common: DecodedCommon) -> Result<Self, Self::Error> {
        match common {
            DecodedCommon::ConnectionRequest { packet } => {
                Ok(DecodedForServer::ConnectionRequest { packet })
            }
            DecodedCommon::ConnectionAccepted { packet } => Err(ProtocolError::InvalidPacketType(
                packet.message.meta.packet_type,
            )),
            DecodedCommon::DataTransfer { packet } => Ok(DecodedForServer::DataTransfer { packet }),
            DecodedCommon::Fragment { packet } => Ok(DecodedForServer::Fragment { packet }),
            DecodedCommon::Heartbeat { packet } => Ok(DecodedForServer::Heartbeat { packet }),
        }
    }
}

impl TryFrom<DecodedCommon> for DecodedForClient {
    type Error = ProtocolError;

    fn try_from(common: DecodedCommon) -> Result<Self, Self::Error> {
        match common {
            DecodedCommon::ConnectionRequest { packet } => Err(ProtocolError::InvalidPacketType(
                packet.message.meta.packet_type,
            )),
            DecodedCommon::ConnectionAccepted { packet } => {
                Ok(DecodedForClient::ConnectionAccepted { packet })
            }
            DecodedCommon::DataTransfer { packet } => Ok(DecodedForClient::DataTransfer { packet }),
            DecodedCommon::Fragment { packet } => Ok(DecodedForClient::Fragment { packet }),
            DecodedCommon::Heartbeat { packet } => Ok(DecodedForClient::Heartbeat { packet }),
        }
    }
}

// impl RawPacket<Server> {
//     pub(crate) fn decode(self, time: Duration) -> Result<DecodedForServer, ProtocolError> {
//         let decoded = self.inner_decode(time)?.try_into()?;
//         Ok(decoded)
//     }
// }

// impl RawPacket<Client> {
//     pub(crate) fn decode(self, time: Duration) -> Result<DecodedForClient, ProtocolError> {
//         let decoded = self.inner_decode(time)?.try_into()?;
//         Ok(decoded)
//     }
// }

impl RawPacket {
    pub fn new(address: SocketAddr, bytes: Vec<u8>) -> Self {
        Self { address, bytes }
    }

    /// TODO(alex) #3 [mid] 2021-08-20: Investigate using the crc32 for Header + a small slice of
    /// the payload, instead of the full thing.
    pub(crate) fn decode<S: Service>(
        self,
        time: Duration,
    ) -> Result<S::DecodedPacketType, ProtocolError> {
        let mut hasher = Hasher::new();

        let length = self.bytes.len();
        let bytes = self.bytes;

        let crc32_position = length - size_of::<u32>();
        let crc32_bytes: &[u8; size_of::<NonZeroU32>()] = bytes[crc32_position..].try_into()?;
        let read_crc32 = u32::from_be_bytes(*crc32_bytes);

        // NOTE(alex): Cannot use the full buffer when re-calculating the crc32 for comparison, as
        // the crc32 is calculated after encoding.
        let bytes_without_crc32 = &bytes[..crc32_position];

        let bytes_with_protocol_id = [&PROTOCOL_ID_BYTES, bytes_without_crc32].concat();

        hasher.update(&bytes_with_protocol_id);
        let crc32 = hasher.finalize();

        if crc32 == read_crc32 {
            let buffer = bytes_with_protocol_id;
            let mut buffer_position = 0;

            // TODO(alex) [low] 2021-08-04: Find out a way to make `from_be_bytes::<ProtocolId>`
            // work.
            //
            // ADD(alex) [low] 2021-08-07: I've tried tackling this to see if a simple wrapper type
            // around `NonZero` would be enough, but it gets a bit too messy for little benefit.
            let read_protocol_id = read_buffer_inc!({buffer, buffer_position } : u32);
            if PROTOCOL_ID.get() != read_protocol_id {
                return Err(ProtocolError::InvalidProtocolId {
                    got: read_protocol_id,
                    expected: PROTOCOL_ID,
                });
            }

            let read_sequence = read_buffer_inc!({ buffer, buffer_position } : u32);
            debug_assert_ne!(read_sequence, 0);

            let read_ack = read_buffer_inc!({ buffer, buffer_position } : Ack);
            let read_packet_type = read_buffer_inc!({ buffer, buffer_position } : PacketType);
            debug_assert!(PACKET_TYPE_RANGE.contains(&read_packet_type));

            let partial_decode = PartialDecode {
                buffer,
                buffer_position,
                packet_type: read_packet_type.try_into()?,
                sequence: read_sequence.try_into()?,
                ack: read_ack,
            };

            let decoded = RawPacket::common_decode::<
                { S::ConnectionPacketType::PACKET_TYPE },
                S::DecodedPacketType,
            >(self.address, partial_decode, time)?;

            let service_decoded = decoded.try_into()?;

            Ok(service_decoded)
        } else {
            Err(ProtocolError::InvalidCrc32 {
                got: read_crc32,
                expected: crc32.try_into()?,
            })
        }
    }

    fn common_decode<const CONNECTION_PACKET: PacketType, T: TryFrom<DecodedCommon>>(
        address: SocketAddr,
        partial_decode: PartialDecode,
        time: Duration,
    ) -> Result<T, ProtocolError> {
        let PartialDecode {
            buffer,
            mut buffer_position,
            packet_type,
            sequence,
            ack,
        } = partial_decode;

        match packet_type {
            // TODO(alex) [high] 2021-08-24: Cannot do this, investigate why:
            // https://rust-lang.github.io/rfcs/1445-restrict-constants-in-patterns.html
            // It would be great if we could reduce the possible packet type pattern matching here,
            // to only accept the correct values depending on the service type.
            CONNECTION_PACKET => {
                todo!()
            }
            ConnectionRequest::PACKET_TYPE => {
                debug!("server: decoding connection request packet.");
                debug_assert_eq!(buffer_position, ConnectionRequest::HEADER_SIZE);

                let delivery = Received {
                    meta: MetaDelivery { time, address },
                };
                let message = ConnectionRequest {
                    meta: MetaMessage { packet_type },
                };

                let packet = Packet {
                    delivery,
                    sequence,
                    ack,
                    message,
                };

                let common = DecodedCommon::ConnectionRequest { packet };
                let decoded = T::try_from(common).unwrap();

                Ok(DecodedCommon::ConnectionRequest { packet })
            }
            ConnectionAccepted::PACKET_TYPE => {
                debug!("client: decoding connection accepted packet.");

                // TODO(alex) #2 [low] 2021-08-04: Find out a way to make
                // `from_be_bytes::<ConnectionId>` work.
                let read_connection_id = read_buffer_inc!({ buffer, buffer_position } : u16);
                debug_assert_eq!(buffer_position, ConnectionAccepted::HEADER_SIZE);

                let delivery = Received {
                    meta: MetaDelivery { time, address },
                };
                let message = ConnectionAccepted {
                    meta: MetaMessage { packet_type },
                    connection_id: read_connection_id.try_into()?,
                };

                let packet = Packet {
                    delivery,
                    sequence,
                    ack,
                    message,
                };

                Ok(DecodedCommon::ConnectionAccepted { packet })
            }
            DataTransfer::PACKET_TYPE => {
                debug!("client: decoding data transfer packet.");

                // TODO(alex) #2 [low] 2021-08-04: Find out a way to make
                // `from_be_bytes::<ConnectionId>` work.
                let read_connection_id = read_buffer_inc!({ buffer, buffer_position } : u16);
                debug_assert_eq!(buffer_position, DataTransfer::HEADER_SIZE);

                let read_payload = buffer[buffer_position..].to_vec();

                let delivery = Received {
                    meta: MetaDelivery { time, address },
                };
                let message = DataTransfer {
                    meta: MetaMessage { packet_type },
                    connection_id: read_connection_id.try_into()?,
                    payload: Arc::new(read_payload),
                };

                let packet = Packet {
                    delivery,
                    sequence,
                    ack,
                    message,
                };

                Ok(DecodedCommon::DataTransfer { packet })
            }
            Fragment::PACKET_TYPE => {
                debug!("client: decoding fragment packet.");

                let read_fragment_index = read_buffer_inc!({ buffer, buffer_position } : u8);
                let read_fragment_total = read_buffer_inc!({ buffer, buffer_position } : u8);
                let read_connection_id = read_buffer_inc!({ buffer, buffer_position } : u16);
                debug_assert_eq!(buffer_position, Fragment::HEADER_SIZE);

                let read_payload = buffer[buffer_position..].to_vec();

                let delivery = Received {
                    meta: MetaDelivery { time, address },
                };
                let message = Fragment {
                    meta: MetaMessage { packet_type },
                    index: read_fragment_index,
                    total: read_fragment_total,
                    connection_id: read_connection_id.try_into()?,
                    payload: Arc::new(read_payload),
                };

                let packet = Packet {
                    delivery,
                    sequence,
                    ack,
                    message,
                };

                Ok(DecodedCommon::Fragment { packet })
            }
            Heartbeat::PACKET_TYPE => {
                debug!("client: decoding heartbeat packet.");

                // TODO(alex) #2 [low] 2021-08-04: Find out a way to make
                // `from_be_bytes::<ConnectionId>` work.
                let read_connection_id = read_buffer_inc!({ buffer, buffer_position } : u16);
                debug_assert_eq!(buffer_position, Heartbeat::HEADER_SIZE);

                let delivery = Received {
                    meta: MetaDelivery { time, address },
                };
                let message = Heartbeat {
                    meta: MetaMessage { packet_type },
                    connection_id: read_connection_id.try_into()?,
                };

                let packet = Packet {
                    delivery,
                    sequence,
                    ack,
                    message,
                };

                Ok(DecodedCommon::Heartbeat { packet })
            }
            invalid => {
                error!("client: decoding invalid packet type {:#?}.", invalid);
                Err(ProtocolError::InvalidPacketType(invalid))
            }
        }
    }
}

pub trait Deliver {}
pub trait Messager {
    const PACKET_TYPE: PacketType;
    const PACKET_TYPE_BYTES: [u8; 1];
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
    pub delivery: Delivery,
    pub sequence: Sequence,
    pub ack: Ack,
    pub message: Message,
}

// REGION(alex): Packet `Delivery` types:
#[derive(Debug, Clone, PartialEq)]
pub struct MetaDelivery {
    pub time: Duration,
    pub address: SocketAddr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToSend {
    pub id: PacketId,
    pub meta: MetaDelivery,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    pub id: PacketId,
    /// NOTE(alex): The time to live helps dealing with `Reliable` packets. If there was no `ttl`,
    /// then packets could remain "unacked" forever.
    ///
    /// Ignored for `Unreliable` packets, as the protocol doesn't store those.
    pub ttl: Duration,
    pub meta: MetaDelivery,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Received {
    pub meta: MetaDelivery,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Acked {
    pub id: PacketId,
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

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionRequest {
    pub meta: MetaMessage,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionAccepted {
    pub meta: MetaMessage,
    pub connection_id: ConnectionId,
}

#[derive(Clone, PartialEq)]
pub struct DataTransfer {
    pub meta: MetaMessage,
    pub connection_id: ConnectionId,
    pub payload: Arc<Payload>,
}

impl core::fmt::Debug for DataTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataTransfer")
            .field("meta", &self.meta)
            .field("connection_id", &self.connection_id)
            .field("payload (length)", &self.payload.len())
            .finish()
    }
}

/// NOTE(alex): The fragment `sequence` is used as its id, making it impossible to ack a single one.
/// In order to ack it, a group of fragments is treated as the actual packet.
#[derive(Clone, PartialEq)]
pub struct Fragment {
    pub meta: MetaMessage,
    pub connection_id: ConnectionId,
    // TODO(alex) [mid] 2021-08-19: To avoid dealing with a `fragment_id`, I'll be treating the
    // `sequence` as its id. We're treating a group of fragments as 1 single packet, so we either
    // receive the whole group and it becomes 1 fully formed packet (converting into a
    // `DataTransfer`), or we discard the parts after they pass some `ttl`.
    //
    // This means that the sequence tracker for a `Peer` only increases after the last fragment is
    // sent out.
    // pub fragment_id: u64,
    pub index: u8,
    pub total: u8,
    pub payload: Arc<Payload>,
}

impl core::fmt::Debug for Fragment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fragment")
            .field("meta", &self.meta)
            .field("connection_id", &self.connection_id)
            // .field("id", &self.fragment_id)
            .field("index", &self.index)
            .field("total", &self.total)
            .field("payload (length)", &self.payload.len())
            .finish()
    }
}

impl Fragment {
    pub(crate) fn new(
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        fragment_index: usize,
        fragment_total: usize,
    ) -> Self {
        let meta = MetaMessage {
            packet_type: Self::PACKET_TYPE,
        };
        let fragment = Self {
            meta,
            connection_id,
            index: fragment_index as u8,
            total: fragment_total as u8,
            payload,
        };

        fragment
    }
}

impl From<Vec<Packet<Received, Fragment>>> for Packet<Received, DataTransfer> {
    fn from(mut fragments: Vec<Packet<Received, Fragment>>) -> Self {
        let last_fragment = fragments.pop().expect("Fragment must exist!");

        let delivery = Received {
            meta: last_fragment.delivery.meta,
        };
        let message = DataTransfer {
            meta: MetaMessage {
                packet_type: DataTransfer::PACKET_TYPE,
            },
            connection_id: last_fragment.message.connection_id,
            payload: Arc::new(fragments.into_iter().fold(
                Vec::with_capacity(last_fragment.message.total as usize * 1500),
                |payload, fragment| {
                    vec![
                        payload,
                        Arc::try_unwrap(fragment.message.payload).expect("Only owner!"),
                    ]
                    .concat()
                },
            )),
        };

        let packet = Packet {
            delivery,
            sequence: last_fragment.sequence,
            ack: last_fragment.ack,
            message,
        };

        packet
    }
}

impl DataTransfer {
    pub(crate) fn new(connection_id: ConnectionId, payload: Arc<Payload>) -> Self {
        let meta = MetaMessage {
            packet_type: Self::PACKET_TYPE,
        };
        let data_transfer = Self {
            meta,
            connection_id,
            payload,
        };

        data_transfer
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Heartbeat {
    pub meta: MetaMessage,
    pub connection_id: ConnectionId,
}

impl Heartbeat {
    pub(crate) fn new(connection_id: ConnectionId) -> Self {
        let meta = MetaMessage {
            packet_type: Self::PACKET_TYPE,
        };
        let heartbeat = Self {
            meta,
            connection_id,
        };

        heartbeat
    }
}

pub const PACKET_TYPE_RANGE: Range<u8> = (PACKET_TYPE_SENTINEL_START + 1)..PACKET_TYPE_SENTINEL_END;
pub const PACKET_TYPE_SENTINEL_START: u8 = 0x0;
pub const CONNECTION_REQUEST: PacketType = unsafe { PacketType::new_unchecked(0x1) };
pub const CONNECTION_ACCEPTED: PacketType = unsafe { PacketType::new_unchecked(0x2) };
pub const DATA_TRANSFER: PacketType = unsafe { PacketType::new_unchecked(0x3) };
pub const FRAGMENT: PacketType = unsafe { PacketType::new_unchecked(0x4) };
pub const HEARTBEAT: PacketType = unsafe { PacketType::new_unchecked(0x5) };
pub const PACKET_TYPE_SENTINEL_END: u8 = 0x6;

impl Messager for ConnectionRequest {
    // TODO(alex) [low] 2021-08-05: Figure out a way to make this incompatible between the Message
    // types. Right now these constants are still programmer enforced, the compiler will happily
    // accept any when we create a packet.
    //
    // One easy way of enforcing the correct packet types would be to use a `new()` function, but
    // I think it would still be a pretty lame solution.
    const PACKET_TYPE: PacketType = CONNECTION_REQUEST;
    const PACKET_TYPE_BYTES: [u8; 1] = Self::PACKET_TYPE.to_be_bytes();
}

impl Messager for ConnectionAccepted {
    const PACKET_TYPE: PacketType = CONNECTION_ACCEPTED;
    const PACKET_TYPE_BYTES: [u8; 1] = Self::PACKET_TYPE.to_be_bytes();
}

impl Messager for DataTransfer {
    const PACKET_TYPE: PacketType = DATA_TRANSFER;
    const PACKET_TYPE_BYTES: [u8; 1] = Self::PACKET_TYPE.to_be_bytes();
}

impl Messager for Fragment {
    const PACKET_TYPE: PacketType = FRAGMENT;
    const PACKET_TYPE_BYTES: [u8; 1] = Self::PACKET_TYPE.to_be_bytes();
}
impl Messager for Heartbeat {
    const PACKET_TYPE: PacketType = HEARTBEAT;
    const PACKET_TYPE_BYTES: [u8; 1] = Self::PACKET_TYPE.to_be_bytes();
}

impl Encoder for ConnectionRequest {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = Self::PACKET_TYPE_BYTES.to_vec();
        debug_assert_eq!(packet_type_bytes.len(), size_of::<PacketType>());

        packet_type_bytes
    }
}

impl Encoder for ConnectionAccepted {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>()
        + size_of::<ConnectionId>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = Self::PACKET_TYPE_BYTES.as_ref();
        let connection_id_bytes = self.connection_id.get().to_be_bytes();

        let encoded = [packet_type_bytes, connection_id_bytes.as_ref()].concat();
        debug_assert_eq!(
            encoded.len(),
            size_of::<PacketType>() + size_of::<ConnectionId>()
        );

        encoded
    }
}

impl Encoder for DataTransfer {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>()
        + size_of::<ConnectionId>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = Self::PACKET_TYPE_BYTES.as_ref();
        let connection_id_bytes = self.connection_id.get().to_be_bytes();
        let payload = self.payload.as_slice();

        // TODO(alex) [low] 2021-08-20: Decreased number of allocations, and hopefully increased
        // performance, according to benchmarks this is the fastest way to concatenate into a vec.
        //
        // Now the question is, do I need to return `Vec<u8>`, or could I return `&[u8]`?
        // The enconding function needs this as a vec?
        let encoded = [packet_type_bytes, connection_id_bytes.as_ref(), payload].concat();
        debug_assert_eq!(
            encoded.len(),
            size_of::<PacketType>() + size_of::<ConnectionId>() + payload.len()
        );

        encoded
    }
}

impl Encoder for Fragment {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>()
        + size_of::<u8>()
        + size_of::<u8>()
        + size_of::<ConnectionId>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = Self::PACKET_TYPE_BYTES.as_ref();
        let fragment_index_bytes = self.index.to_be_bytes();
        let fragment_total_bytes = self.total.to_be_bytes();
        let connection_id_bytes = self.connection_id.get().to_be_bytes();
        let payload = self.payload.as_slice();

        let encoded = [
            packet_type_bytes,
            fragment_index_bytes.as_ref(),
            fragment_total_bytes.as_ref(),
            connection_id_bytes.as_ref(),
            payload,
        ]
        .concat();
        debug_assert_eq!(
            encoded.len(),
            size_of::<PacketType>()
                + size_of::<u8>()
                + size_of::<u8>()
                + size_of::<ConnectionId>()
                + payload.len()
        );

        encoded
    }
}

impl Encoder for Heartbeat {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>()
        + size_of::<ConnectionId>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = Self::PACKET_TYPE_BYTES.as_ref();
        let connection_id_bytes = self.connection_id.get().to_be_bytes();

        let encoded = [packet_type_bytes, connection_id_bytes.as_ref()].concat();
        debug_assert_eq!(
            encoded.len(),
            size_of::<PacketType>() + size_of::<ConnectionId>()
        );

        encoded
    }
}

impl<Message> Packet<ToSend, Message>
where
    Message: Messager + Encoder,
{
    pub fn sent(self, time: Duration, ttl: Duration) -> Packet<Sent, Message> {
        let packet = Packet {
            delivery: Sent {
                id: self.delivery.id,
                ttl,
                meta: MetaDelivery {
                    time,
                    address: self.delivery.meta.address,
                },
            },
            sequence: self.sequence,
            ack: self.ack,
            message: self.message,
        };

        packet
    }

    pub fn as_raw<T>(&self) -> RawPacket {
        let sequence_bytes = self.sequence.get().to_be_bytes();
        let ack_bytes = self.ack.to_be_bytes();

        let encoded_message = self.message.encoded();

        let mut hasher = Hasher::new();
        let mut bytes = [
            PROTOCOL_ID_BYTES.as_ref(),
            sequence_bytes.as_ref(),
            ack_bytes.as_ref(),
            &encoded_message,
        ]
        .concat();

        hasher.update(&bytes);
        let crc32 = hasher.finalize();
        debug_assert!(crc32 != 0);

        bytes.append(&mut crc32.to_be_bytes().to_vec());

        let packet_bytes = bytes[size_of::<ProtocolId>()..].to_vec();
        let raw_packet = RawPacket {
            address: self.delivery.meta.address,
            bytes: packet_bytes,
        };

        // debug_assert_eq!(self, raw_packet.decode()?)

        raw_packet
    }
}

// TODO(alex) [mid] 2021-08-19: Create a derive macro for the `into_packet` implementation, it's the
// same for every kind of packet.
#[derive(Debug, Clone, PartialEq)]
pub struct Scheduled<R: Reliability, Message: Messager> {
    pub packet_id: PacketId,
    pub address: SocketAddr,
    pub time: Duration,
    pub reliability: R,
    pub message: Message,
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum ReliabilityType {
    Reliable,
    Unreliable,
}

pub trait Reliability {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reliable {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreliable {}

impl Reliability for Reliable {}

impl Reliability for Unreliable {}

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
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
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
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }
}

impl Scheduled<Unreliable, DataTransfer> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, DataTransfer> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }
}

impl Scheduled<Reliable, DataTransfer> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, DataTransfer> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }
}

impl Scheduled<Unreliable, Fragment> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, Fragment> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }

    pub(crate) fn new_unreliable_fragment(
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        fragment_index: usize,
        fragment_total: usize,
        time: Duration,
        address: SocketAddr,
    ) -> Self {
        let scheduled = Self {
            packet_id,
            time,
            address,
            reliability: Unreliable {},
            message: Fragment::new(connection_id, payload, fragment_index, fragment_total),
        };

        scheduled
    }
}

impl Scheduled<Reliable, Fragment> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, Fragment> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }

    pub(crate) fn new_reliable_fragment(
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        fragment_index: usize,
        fragment_total: usize,
        time: Duration,
        address: SocketAddr,
    ) -> Self {
        let scheduled = Self {
            packet_id,
            time,
            address,
            reliability: Reliable {},
            message: Fragment::new(connection_id, payload, fragment_index, fragment_total),
        };

        scheduled
    }
}

impl Scheduled<Unreliable, Heartbeat> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, Heartbeat> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }
}

impl Scheduled<Reliable, Heartbeat> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, Heartbeat> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }
}

impl Scheduled<Unreliable, DataTransfer> {
    pub(crate) fn new_unreliable_data_transfer(
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        time: Duration,
        address: SocketAddr,
    ) -> Self {
        let scheduled = Self {
            packet_id,
            address,
            time,
            reliability: Unreliable {},
            message: DataTransfer::new(connection_id, payload),
        };

        scheduled
    }
}

impl Scheduled<Reliable, DataTransfer> {
    pub(crate) fn new_reliable_data_transfer(
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        time: Duration,
        address: SocketAddr,
    ) -> Self {
        let scheduled = Self {
            packet_id,
            address,
            time,
            reliability: Reliable {},
            message: DataTransfer::new(connection_id, payload),
        };

        scheduled
    }
}

impl Scheduled<Unreliable, Heartbeat> {
    pub(crate) fn new_unreliable_heartbeat(
        packet_id: PacketId,
        connection_id: ConnectionId,
        time: Duration,
        address: SocketAddr,
    ) -> Self {
        let scheduled = Self {
            packet_id,
            address,
            time,
            reliability: Unreliable {},
            message: Heartbeat::new(connection_id),
        };

        scheduled
    }
}

impl Scheduled<Reliable, Heartbeat> {
    pub(crate) fn new_reliable_heartbeat(
        packet_id: PacketId,
        connection_id: ConnectionId,
        time: Duration,
        address: SocketAddr,
    ) -> Self {
        let scheduled = Self {
            packet_id,
            address,
            time,
            reliability: Reliable {},
            message: Heartbeat::new(connection_id),
        };

        scheduled
    }
}
