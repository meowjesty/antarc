use std::{
    convert::TryInto,
    marker::PhantomData,
    mem::size_of,
    net::SocketAddr,
    num::NonZeroU32,
    time::{Duration, Instant},
};

use crc32fast::Hasher;

use super::{
    header::{ConnectionRequest, DataTransfer, Generic, Header, HeaderInfo, Heartbeat},
    payload::Payload,
    ConnectionId, Footer, Packet, Sent,
};
use crate::{
    events::ProtocolError,
    packet::{
        header::ENCODED_SIZE, sequence::Sequence, Ack, PacketKind, StatusCode, CONNECTION_REQUEST,
    },
    read_buffer_inc, ProtocolId, PROTOCOL_ID, PROTOCOL_ID_BYTES,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Received<Kind> {
    pub header: Header<Kind>,
    pub footer: Footer,
    pub time: Duration,
    pub source: SocketAddr,
}

impl Packet<Received<Generic>> {
    // TODO(alex) 2021-05-17: Check that this code is working by comparing it with the ECS branch,
    // I don't remember if this encode was in a proper working state when the `restart` branch was
    // created.
    pub fn decode(
        id: u64,
        buffer: &[u8],
        address: SocketAddr,
        timer: &Instant,
    ) -> Result<(Packet<Received<Generic>>, Payload), ProtocolError> {
        let mut hasher = Hasher::new();

        let buffer_length = buffer.len();

        let crc32_position = buffer_length - size_of::<NonZeroU32>();

        let crc32_bytes: &[u8; size_of::<NonZeroU32>()] = buffer[crc32_position..].try_into()?;
        let crc32_received = u32::from_be_bytes(*crc32_bytes);

        // NOTE(alex): Cannot use the full buffer when recalculating the crc32 for comparison, as
        // the crc32 is calculated after encoding.
        let buffer_without_crc32 = &buffer[..crc32_position];

        let packet_with_protocol_id = [&PROTOCOL_ID_BYTES, buffer_without_crc32].concat();

        hasher.update(&packet_with_protocol_id);
        let crc32 = hasher.finalize();

        if crc32 == crc32_received {
            let buffer = packet_with_protocol_id;
            let mut buffer_position = 0;
            // TODO(alex) 2021-04-01: Find out a way to `from_be_bytes::<NonZeroU32>` to work.

            let read_protocol_id = read_buffer_inc!({buffer, buffer_position } : u32);
            if PROTOCOL_ID.get() != read_protocol_id {
                return Err(ProtocolError::InvalidProtocolId {
                    got: read_protocol_id,
                    expected: PROTOCOL_ID,
                });
            }

            let read_sequence = read_buffer_inc!({buffer, buffer_position } : u32);
            debug_assert_ne!(read_sequence, 0);

            let read_ack = read_buffer_inc!({buffer, buffer_position } : Ack);
            let read_past_acks = read_buffer_inc!({buffer, buffer_position } : u16);
            let read_status_code = read_buffer_inc!({buffer, buffer_position } : StatusCode);
            let read_payload_length = read_buffer_inc!({buffer, buffer_position } : u16);
            debug_assert_eq!(buffer_position, ENCODED_SIZE);

            let sequence = Sequence(read_sequence.try_into().unwrap());
            let header_info = HeaderInfo {
                sequence,
                ack: read_ack,
                past_acks: read_past_acks,
                status_code: read_status_code,
                payload_length: read_payload_length,
            };

            let payload_length = read_payload_length as usize;
            let read_payload = buffer[buffer_position..buffer_position + payload_length].to_vec();
            buffer_position += payload_length;
            debug_assert_eq!(buffer_position, ENCODED_SIZE + payload_length);
            let payload = Payload(read_payload);

            // TODO(alex) 2021-04-29: Change this naked number into something meaningful before it
            // bites me in the debug assertion again.
            let connection_id = if read_status_code > CONNECTION_REQUEST {
                let read_connection_id = read_buffer_inc!({buffer, buffer_position} : u16);
                debug_assert_ne!(read_connection_id, 0);
                debug_assert_eq!(
                    buffer_position,
                    ENCODED_SIZE + payload_length + size_of::<ConnectionId>()
                );
                Some(read_connection_id.try_into()?)
            } else {
                None
            };
            let footer = Footer {
                connection_id,
                crc32: crc32.try_into()?,
            };

            let header = Header {
                info: header_info,
                marker: PhantomData::default(),
            };
            let state = Received {
                header,
                footer,
                time: timer.elapsed(),
                source: address,
            };

            let packet = Packet { id, state };

            Ok((packet, payload))
        } else {
            Err(ProtocolError::InvalidCrc32 {
                got: crc32_received,
                expected: unsafe { NonZeroU32::new_unchecked(crc32) },
            })
        }
    }
}

impl From<Packet<Received<Generic>>> for Packet<Received<DataTransfer>> {
    fn from(packet: Packet<Received<Generic>>) -> Self {
        let header = Header {
            info: packet.state.header.info,
            marker: PhantomData::default(),
        };
        let state = Received {
            header,
            footer: packet.state.footer,
            time: packet.state.time,
            source: packet.state.source,
        };
        let packet = Packet {
            id: packet.id,
            state,
        };

        packet
    }
}

impl From<Packet<Received<Generic>>> for Packet<Received<ConnectionRequest>> {
    fn from(packet: Packet<Received<Generic>>) -> Self {
        let header = Header {
            info: packet.state.header.info,
            marker: PhantomData::default(),
        };
        let state = Received {
            header,
            footer: packet.state.footer,
            time: packet.state.time,
            source: packet.state.source,
        };
        let packet = Packet {
            id: packet.id,
            state,
        };

        packet
    }
}

impl From<Packet<Received<Generic>>> for Packet<Received<Heartbeat>> {
    fn from(packet: Packet<Received<Generic>>) -> Self {
        let header = Header {
            info: packet.state.header.info,
            marker: PhantomData::default(),
        };
        let state = Received {
            header,
            footer: packet.state.footer,
            time: packet.state.time,
            source: packet.state.source,
        };
        let packet = Packet {
            id: packet.id,
            state,
        };

        packet
    }
}
