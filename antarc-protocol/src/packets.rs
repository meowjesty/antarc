use core::{mem::size_of, ops::RangeInclusive, time::Duration};
use std::{
    convert::TryInto,
    marker::PhantomData,
    net::SocketAddr,
    num::{NonZeroU16, NonZeroU32},
};

use crc32fast::Hasher;
use log::{debug, error, warn};

use crate::{
    client::Client,
    events::{ProtocolError, ScheduleEvent},
    read_buffer_inc,
    server::Server,
    ProtocolId, PROTOCOL_ID, PROTOCOL_ID_BYTES,
};

pub type PacketId = u64;
pub type Sequence = NonZeroU32;
pub type ConnectionId = NonZeroU16;
pub type Ack = u32;
pub type PacketType = u8;
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
pub struct RawPacket<T> {
    pub address: SocketAddr,
    pub bytes: Vec<u8>,
    pub phantom: PhantomData<T>,
}

// TODO(alex) [vlow] 2021-08-05: Maybe renaming the connection packets to some handshake-y name
// would make things clearer.
#[derive(Debug, Clone, PartialEq)]
pub enum Decoded {
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

pub const PACKET_TYPE_RANGE: RangeInclusive<u8> = 0x1..=0x6;

impl RawPacket<Server> {
    pub(crate) fn decode(self, time: Duration) -> Result<DecodedForServer, ProtocolError> {
        let address = self.address;
        let PartialDecode {
            buffer,
            mut buffer_position,
            packet_type,
            sequence,
            ack,
        } = self.inner_decode()?;

        match packet_type {
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

                Ok(DecodedForServer::ConnectionRequest { packet })
            }
            ConnectionAccepted::PACKET_TYPE => {
                warn!("server: tried decoding a connection accepted, skipping.");
                Err(ProtocolError::InvalidPacketType(packet_type))
            }
            DataTransfer::PACKET_TYPE => {
                debug!("server: decoding data transfer packet.");

                // TODO(alex) [low] 2021-08-04: Find out a way to make
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
                    payload: read_payload,
                };

                let packet = Packet {
                    delivery,
                    sequence,
                    ack,
                    message,
                };

                Ok(DecodedForServer::DataTransfer { packet })
            }
            Fragment::PACKET_TYPE => {
                debug!("server: decoding fragment packet.");
                todo!();
            }
            Heartbeat::PACKET_TYPE => {
                debug!("server: decoding heartbeat packet.");
                todo!();
            }
            invalid => {
                error!("server: decoding invalid packet type {:#?}.", invalid);
                Err(ProtocolError::InvalidPacketType(invalid).into())
            }
        }
    }
}

impl RawPacket<Client> {
    pub(crate) fn decode(self, time: Duration) -> Result<DecodedForClient, ProtocolError> {
        let address = self.address;
        let PartialDecode {
            buffer,
            mut buffer_position,
            packet_type,
            sequence,
            ack,
        } = self.inner_decode()?;

        match packet_type {
            ConnectionRequest::PACKET_TYPE => {
                warn!("client: tried decoding a connection request, skipping.");
                Err(ProtocolError::InvalidPacketType(packet_type))
            }
            ConnectionAccepted::PACKET_TYPE => {
                debug!("client: decoding connection accepted packet.");

                // TODO(alex) [low] 2021-08-04: Find out a way to make
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

                Ok(DecodedForClient::ConnectionAccepted { packet })
            }
            DataTransfer::PACKET_TYPE => {
                debug!("client: decoding data transfer packet.");

                // TODO(alex) [low] 2021-08-04: Find out a way to make
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
                    payload: read_payload,
                };

                let packet = Packet {
                    delivery,
                    sequence,
                    ack,
                    message,
                };

                Ok(DecodedForClient::DataTransfer { packet })
            }
            Fragment::PACKET_TYPE => {
                debug!("client: decoding fragment packet.");
                todo!();
            }
            Heartbeat::PACKET_TYPE => {
                debug!("client: decoding heartbeat packet.");
                todo!();
            }
            invalid => {
                error!("client: decoding invalid packet type {:#?}.", invalid);
                Err(ProtocolError::InvalidPacketType(invalid).into())
            }
        }
    }
}

impl<T> RawPacket<T> {
    pub fn new(address: SocketAddr, bytes: Vec<u8>) -> Self {
        Self {
            address,
            bytes,
            phantom: PhantomData::default(),
        }
    }

    pub(crate) fn inner_decode(self) -> Result<PartialDecode, ProtocolError> {
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
                }
                .into());
            }

            let read_sequence = read_buffer_inc!({ buffer, buffer_position } : u32);
            debug_assert_ne!(read_sequence, 0);

            let read_ack = read_buffer_inc!({ buffer, buffer_position } : Ack);
            let read_packet_type = read_buffer_inc!({ buffer, buffer_position } : PacketType);
            debug_assert!(PACKET_TYPE_RANGE.contains(&read_packet_type));

            let partial_decode = PartialDecode {
                buffer,
                buffer_position,
                packet_type: read_packet_type,
                sequence: read_sequence.try_into()?,
                ack: read_ack,
            };

            Ok(partial_decode)
        } else {
            Err(ProtocolError::InvalidCrc32 {
                got: read_crc32,
                expected: crc32.try_into()?,
            })
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

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionRequest {
    pub meta: MetaMessage,
}

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

impl Fragment {
    pub(crate) fn new(
        connection_id: ConnectionId,
        payload: Payload,
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
            payload: payload.clone(),
        };

        fragment
    }
}

impl DataTransfer {
    pub(crate) fn new(connection_id: ConnectionId, payload: Payload) -> Self {
        let meta = MetaMessage {
            packet_type: Self::PACKET_TYPE,
        };
        let data_transfer = Self {
            meta,
            connection_id,
            payload: payload.clone(),
        };

        data_transfer
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Heartbeat {
    pub meta: MetaMessage,
    pub connection_id: ConnectionId,
}

pub const CONNECTION_REQUEST: PacketType = 0x1;
pub const CONNECTION_ACCEPTED: PacketType = 0x2;
pub const DATA_TRANSFER: PacketType = 0x3;
pub const FRAGMENT: PacketType = 0x4;
pub const HEARTBEAT: PacketType = 0x5;

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
        let packet_type_bytes = Self::PACKET_TYPE_BYTES.to_vec();
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
        + size_of::<ConnectionId>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = Self::PACKET_TYPE_BYTES.to_vec();
        let connection_id_bytes = self.connection_id.get().to_be_bytes().to_vec();
        let payload = self.payload.clone();

        let encoded = vec![packet_type_bytes, connection_id_bytes, payload].concat();
        encoded
    }
}

impl Encoder for Fragment {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>()
        // TODO(alex) [mid] 2021-08-04: Missing fragmend index, fragment total sizes.
        + size_of::<ConnectionId>();

    fn encoded(&self) -> Vec<u8> {
        todo!();
        // let packet_type_bytes = Self::PACKET_TYPE_BYTES.to_vec();

        // packet_type_bytes
    }
}
impl Encoder for Heartbeat {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>()
        + size_of::<ConnectionId>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = Self::PACKET_TYPE_BYTES.to_vec();
        let connection_id_bytes = self.connection_id.get().to_be_bytes().to_vec();

        let encoded = vec![packet_type_bytes, connection_id_bytes].concat();
        encoded
    }
}

impl<Message> Packet<ToSend, Message>
where
    Message: Messager + Encoder,
{
    pub fn sent(self, time: Duration) -> Packet<Sent, Message> {
        let packet = Packet {
            delivery: Sent {
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

    pub fn as_raw<T>(&self) -> RawPacket<T> {
        let sequence_bytes = self.sequence.get().to_be_bytes().to_vec();
        let ack_bytes = self.ack.to_be_bytes().to_vec();

        let encoded_message = self.message.encoded();

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
            address: self.delivery.meta.address,
            bytes: packet_bytes,
            phantom: PhantomData::default(),
        };

        // debug_assert_eq!(self, raw_packet.decode()?)

        raw_packet
    }
}

// TODO(alex) [low] 2021-07-31: Instead of having one master type with optional fields + bools, we
// could have a family of `Scheduled`, much like `Packet`, and the `scheduler_pipe` would take an
// enum of such types.
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

impl ReliabilityType {
    pub(crate) fn as_fragment_event(
        self,
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Payload,
        fragment_index: usize,
        fragment_total: usize,
        time: Duration,
        address: SocketAddr,
    ) -> ScheduleEvent {
        // TODO(alex) [mid] 2021-08-08: This and the data transfer part are
        // finnicky, if I call the wrong `new` function, we could end up with both
        // sides creating the same `ScheduleEvent`.
        //
        // Maybe one simple way of solving this would be to put the `Reliable` and
        // `Unreliable` types inside the `ReliabilityType::Reliable(Reliable)` for
        // example, and pass it into the `new` functions.
        match self {
            ReliabilityType::Reliable => {
                let scheduled: Scheduled<Reliable, Fragment> = Scheduled::new_reliable_fragment(
                    packet_id,
                    connection_id,
                    payload,
                    fragment_index,
                    fragment_total,
                    time,
                    address,
                );

                scheduled.into()
            }
            ReliabilityType::Unreliable => {
                let scheduled: Scheduled<Unreliable, Fragment> = Scheduled::new_unreliable_fragment(
                    packet_id,
                    connection_id,
                    payload,
                    fragment_index,
                    fragment_total,
                    time,
                    address,
                );

                scheduled.into()
            }
        }
    }

    pub(crate) fn as_data_transfer_event(
        self,
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Payload,
        time: Duration,
        address: SocketAddr,
    ) -> ScheduleEvent {
        // TODO(alex) [mid] 2021-08-08: This and the fragment part are finnicky, if I call
        // the wrong `new` function, we could end up with both sides creating the same
        // `ScheduleEvent`.
        match self {
            ReliabilityType::Reliable => {
                let scheduled: Scheduled<Reliable, DataTransfer> =
                    Scheduled::new_reliable_data_transfer(
                        packet_id,
                        connection_id,
                        payload,
                        time,
                        address,
                    );

                scheduled.into()
            }
            ReliabilityType::Unreliable => {
                let scheduled: Scheduled<Unreliable, DataTransfer> =
                    Scheduled::new_unreliable_data_transfer(
                        packet_id,
                        connection_id,
                        payload,
                        time,
                        address,
                    );

                scheduled.into()
            }
        }
    }
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

impl Scheduled<Unreliable, Fragment> {
    pub(crate) fn new_unreliable_fragment(
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Payload,
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
    pub(crate) fn new_reliable_fragment(
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Payload,
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

impl Scheduled<Unreliable, DataTransfer> {
    pub(crate) fn new_unreliable_data_transfer(
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Payload,
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
        payload: Payload,
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
