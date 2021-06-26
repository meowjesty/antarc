use std::{convert::TryInto, mem::size_of, net::SocketAddr, num::NonZeroU32};

use crc32fast::Hasher;

use super::{partial::PartialPacket, ConnectionId};
use crate::{
    events::ProtocolError,
    header::{HeaderInfo, ENCODED_SIZE},
    packets::{Ack, StatusCode, CONNECTION_REQUEST},
    payload::Payload,
    read_buffer_inc,
    sequence::Sequence,
    PROTOCOL_ID, PROTOCOL_ID_BYTES,
};

#[derive(Debug, Clone, PartialEq)]
pub struct RawPacket {
    pub address: SocketAddr,
    pub bytes: Vec<u8>,
}

impl RawPacket {
    pub fn new(address: SocketAddr, bytes: Vec<u8>) -> Self {
        Self { address, bytes }
    }

    // TODO(alex) 2021-05-17: Check that this code is working by comparing it with the ECS branch,
    // I don't remember if this encode was in a proper working state when the `restart` branch was
    // created.
    //
    // TODO(alex) [mid] 2021-06-26: This function returns a `PartialPacket` of some sort, which is
    // a packet that contain options (well, just the connection id being an option). This partial
    // is returned (think as a builder object, but not really going that far) and when we know the
    // host that should receive this packet, then we can check for the packet code and decide which
    // kind of "real" packet this is.
    pub fn decode(self, id: u64) -> Result<PartialPacket, ProtocolError> {
        let buffer = self.bytes;
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

            let partial_packet = PartialPacket {
                id,
                header_info,
                payload,
                crc32: crc32.try_into()?,
                connection_id,
                address: self.address,
            };

            Ok(partial_packet)
        } else {
            Err(ProtocolError::InvalidCrc32 {
                got: crc32_received,
                expected: unsafe { NonZeroU32::new_unchecked(crc32) },
            })
        }
    }
}
