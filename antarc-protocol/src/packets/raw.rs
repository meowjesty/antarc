use core::convert::TryInto;
use std::net::SocketAddr;

use crc32fast::Hasher;

use super::*;
use crate::*;

#[derive(Debug, Clone, PartialEq)]
pub struct RawPacket {
    pub address: SocketAddr,
    pub bytes: Vec<u8>,
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

// TODO(alex) [high] 2021-08-25: Read about what is going on here:
// https://github.com/rust-lang/rfcs/blob/master/text/1598-generic_associated_types.md
impl RawPacket {
    pub(crate) fn decode<S>(self, time: Duration) -> Result<S::DecodedType, ProtocolError>
    where
        S: Service,
        <S as service_traits::Service>::DecodedType: TryFrom<decode::DecodedCommon>,
        errors::ProtocolError: From<
            <<S as service_traits::Service>::DecodedType as std::convert::TryFrom<
                packets::decode::DecodedCommon,
            >>::Error,
        >,
    {
        let decoded = self.inner_decode(time)?.try_into()?;
        Ok(decoded)
    }
}

impl RawPacket {
    pub fn new(address: SocketAddr, bytes: Vec<u8>) -> Self {
        Self { address, bytes }
    }

    /// TODO(alex) #3 [mid] 2021-08-20: Investigate using the crc32 for Header + a small slice of
    /// the payload, instead of the full thing.
    pub(crate) fn inner_decode(self, time: Duration) -> Result<DecodedCommon, ProtocolError> {
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

            let decoded = RawPacket::common_decode(self.address, partial_decode, time)?;

            Ok(decoded)
        } else {
            Err(ProtocolError::InvalidCrc32 {
                got: read_crc32,
                expected: crc32.try_into()?,
            })
        }
    }

    fn common_decode(
        address: SocketAddr,
        partial_decode: PartialDecode,
        time: Duration,
    ) -> Result<DecodedCommon, ProtocolError> {
        let PartialDecode {
            buffer,
            mut buffer_position,
            packet_type,
            sequence,
            ack,
        } = partial_decode;

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
