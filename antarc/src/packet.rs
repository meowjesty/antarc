use core::mem::size_of;
use std::{
    convert::TryInto,
    io::{BufRead, Cursor, IoSlice, Read, Write},
    marker::PhantomData,
    num::{NonZeroU16, NonZeroU32, NonZeroU8},
    time::Duration,
};

use crc32fast::Hasher;
use log::debug;

use self::header::Header;
use crate::{
    read_buffer_inc, AntarcResult, PacketMarker, ProtocolId, BUFFER_CAP, END_OF_PACKET_BYTES,
    PACKED_LEN, PROTOCOL_ID, PROTOCOL_ID_BYTES,
};

pub(crate) mod header;

/// Packets might be either:
/// - FRAGMENTED or NON_FRAGMENTED;
/// - DATA_TRANSFER or CONNECTION_REQUEST or CHALLENGE or CHALLENGE_RESPONSE;
///
/// this is valid:
/// - `FRAGMENTED | DATA_TRANSFER`
///
/// but this is NOT:
/// - `FRAGMENTED | DATA_TRANSFER | CHALLENGE`
pub(crate) type StatusCode = u16;
pub(crate) const RESERVED: StatusCode = 0;
// pub(crate) const FRAGMENTED: StatusCode = 1;
pub(crate) const CONNECTION_REQUEST: StatusCode = 100;
pub(crate) const CHALLENGE_REQUEST: StatusCode = 200;
pub(crate) const CHALLENGE_RESPONSE: StatusCode = 300;
pub(crate) const CONNECTION_ACCEPTED: StatusCode = 400;
pub(crate) const CONNECTION_DENIED: StatusCode = 500;
pub(crate) const HEARTBEAT: StatusCode = 600;
pub(crate) const DATA_TRANSFER: StatusCode = 700;

/// TODO(alex) 2021-02-09: Improve terminology:
/// http://www.tcpipguide.com/free/t_MessagesPacketsFramesDatagramsandCells-2.htm
///
/// http://www.tcpipguide.com/free/t_MessageFormattingHeadersPayloadsandFooters.htm
/// TODO(alex) 2021-02-26: The idea of "framing" a packet can be understood as putting it in an
/// envelope, making it clear where the packet ends, starts, where is the message. This will help
/// with packet fragmentation.

/// NOTE(alex): Valid `Packet` state transitions:
/// - Received -> Retrieved;
/// - ToSend -> Sent -> Acked;
pub type ConnectionId = NonZeroU16;
pub type Sequence = NonZeroU32;
pub type Ack = u32;

// REGION(alex) 2021-03-23: Types of packet (event types).
#[derive(Debug)]
pub(crate) struct Queued {
    pub(crate) time: Duration,
}

#[derive(Debug)]
pub(crate) struct Received {
    pub(crate) time: Duration,
}

#[derive(Debug)]
pub(crate) struct Sent {
    pub(crate) time: Duration,
}

#[derive(Debug)]
pub(crate) struct Acked {
    pub(crate) time: Duration,
}

/// NOTE(alex) 2021-01-28: These are packets that were received, and the user application has
/// loaded them, so they're moved into this state.
#[derive(Debug)]
pub(crate) struct Retrieved {
    pub(crate) time: Duration,
}

/// NOTE(alex) 2021-02-06: These packets are intercepted by the protocol and handled internally,
/// they cannot be retrieved by the user. Deals with connection requests (connection handling),
/// hearbeat, fragmentation.
/// TODO(alex) 2021-02-15: This name sucks and doesn't really convey what it does.
#[derive(Debug)]
pub(crate) struct Internal {
    pub(crate) time: Duration,
}

// REGION(alex) 2021-03-23: Types of packet (metadata).
#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct HostPacket {
    host_id: u32,
    packet_id: u32,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct ConnectionRequest;

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct ConnectionDenied;

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct ConnectionAccepted;

/// Identifies the connection, this enables network switching from either side without having
/// to slowly re-estabilish the connection.
#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct DataTransfer;

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct Heartbeat;

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) enum PacketType {
    ConnectionRequest(ConnectionRequest),
    ConnectionDenied(ConnectionDenied),
    ConnectionAccepted(ConnectionAccepted),
    DataTransfer(DataTransfer),
    Heartbeat(Heartbeat),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Payload(pub(crate) Vec<u8>);

impl Payload {
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct Footer {
    pub(crate) connection_id: Option<ConnectionId>,
    pub(crate) crc32: NonZeroU32,
}

impl Footer {
    // pub(crate) const ENCODED_SIZE: usize = size_of::<(ConnectionId, NonZeroU32)>();
}

/// TODO(alex) 2021-02-09: There must be a way to mark a packet as `Reliable` and/or `Priority`.
/// The `Reliable` packet will keep retrying until it is acked, how the algorithm will actually work
/// I'm still unsure, should it keep bumping itself into being the first to send, until it's acked?
/// Maybe send it once, wait some `timeout_not_acked` time and then resend it, this would allow the
/// `past_acks` part to shine, as we could end up not getting a direct packet ack (the client
/// doesn't receive a packet with `ack` equals the sent `sequence`), but it gets acked anyway by
/// the `past_acks` in some later packet saying: "Hey, I've acked the last 6 packets you've sent",
/// this would be done by getting whatever `sequence` the server received (client sent packet 10,
/// but server never got packet 9, 8, 7), acking it, and replying with a
/// `ack: 10, past_ack: (10, 6, 5, 4, 3,...)`, then the client would know that the server is acking
/// the packet 10, but missed some (9, 8, 7).

/// NOTE(alex) 2021-02-01: By using the `State` type parameter, it becomes possible to store
/// whatever struct metadata we want here. Each packet state will hold a different `state` data.
#[derive(Debug)]
// pub(crate) struct Packet;
pub(crate) struct Packet {
    /// TODO(alex) 2021-02-28: How do we actually do this? The crc32 will only be calculated at
    /// encode time, is this `Footer` a phantasm type that will be sent at the end of the
    /// packet (when encoded), but doesn't exist as an actual type here?
    pub(crate) header: Header,
    pub(crate) payload: Payload,
    pub(crate) footer: Footer,
}

impl Packet {
    pub(crate) fn encode(
        header: &Header,
        payload: &Payload,
        connection_id: Option<ConnectionId>,
    ) -> Result<(Vec<u8>, Footer), String> {
        let sequence_bytes = header.sequence.get().to_be_bytes().to_vec();
        let ack_bytes = header.ack.to_be_bytes().to_vec();
        let past_acks_bytes = header.past_acks.to_be_bytes().to_vec();
        let status_code_bytes = header.status_code.to_be_bytes().to_vec();
        let payload_length_bytes = header.payload_length.to_be_bytes().to_vec();
        // TODO(alex) 2021-04-24: crc32 calculation is wrong when encoding, the debug assertion
        // doesn't match.

        let mut hasher = Hasher::new();
        let mut bytes = vec![
            PROTOCOL_ID_BYTES.to_vec(),
            sequence_bytes,
            ack_bytes,
            past_acks_bytes,
            status_code_bytes,
            payload_length_bytes,
            payload.0.clone(),
        ]
        .concat();

        if let Some(connection_id) = connection_id {
            let mut connection_id_bytes = connection_id.get().to_be_bytes().to_vec();
            bytes.append(&mut connection_id_bytes);
        }

        hasher.update(&bytes);
        let crc32 = hasher.finalize();
        debug_assert!(crc32 != 0);

        bytes.append(&mut crc32.to_be_bytes().to_vec());

        let footer = Footer {
            connection_id,
            crc32: unsafe { NonZeroU32::new_unchecked(crc32) },
        };

        let packet_bytes = bytes[size_of::<ProtocolId>()..].to_vec();

        debug_assert!({
            let (read_header, read_payload, read_footer) = Packet::decode(&packet_bytes).unwrap();
            *header == read_header && read_payload == *payload && read_footer == footer
        });

        Ok((packet_bytes, footer))
    }

    /// NOTE(alex) 2021-04-26: This `buffer` should contain only the packet, be careful not to pass
    /// the whole receiver buffer into here.
    pub(crate) fn decode(buffer: &[u8]) -> Result<(Header, Payload, Footer), String> {
        let mut hasher = Hasher::new();

        let buffer_length = buffer.len();
        debug!("length {:?}", buffer_length);

        let crc32_position = buffer_length - size_of::<NonZeroU32>();
        debug!("crc32 position {:?}", crc32_position);

        let crc32_bytes: &[u8; size_of::<NonZeroU32>()] =
            buffer[crc32_position..].try_into().unwrap();
        debug!("crc32 bytes {:?}", crc32_bytes);
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
                return Err(format!(
                    "Decoded invalid protocol id {:#?}.",
                    read_protocol_id
                ));
            }

            let read_sequence = read_buffer_inc!({buffer, buffer_position } : u32);
            debug_assert_ne!(read_sequence, 0);

            let read_ack = read_buffer_inc!({buffer, buffer_position } : Ack);
            let read_past_acks = read_buffer_inc!({buffer, buffer_position } : u16);
            let read_status_code = read_buffer_inc!({buffer, buffer_position } : StatusCode);
            let read_payload_length = read_buffer_inc!({buffer, buffer_position } : u16);
            debug_assert_eq!(buffer_position, Header::ENCODED_SIZE);

            let header = Header {
                sequence: read_sequence.try_into().unwrap(),
                ack: read_ack,
                past_acks: read_past_acks,
                status_code: read_status_code,
                payload_length: read_payload_length,
            };

            let payload_length = read_payload_length as usize;
            let read_payload = buffer[buffer_position..buffer_position + payload_length].to_vec();
            buffer_position += payload_length;
            debug_assert_eq!(buffer_position, Header::ENCODED_SIZE + payload_length);
            let payload = Payload(read_payload);

            // TODO(alex) 2021-04-29: Change this naked number into something meaningful before it
            // bites me in the debug assertion again.
            let connection_id = if read_status_code > 300 {
                let read_connection_id = read_buffer_inc!({buffer, buffer_position} : u16);
                debug_assert_ne!(read_connection_id, 0);
                debug_assert_eq!(
                    buffer_position,
                    Header::ENCODED_SIZE + payload_length + size_of::<ConnectionId>()
                );
                Some(read_connection_id.try_into().unwrap())
            } else {
                None
            };
            let footer = Footer {
                connection_id,
                crc32: crc32.try_into().unwrap(),
            };

            Ok((header, payload, footer))
        } else {
            Err(format!(
                "Received {:#?} crc32, but calculated {:#?}.",
                crc32_received, crc32
            ))
        }
    }

    /// TODO(alex) 2021-02-05: Hash the buffer to have a fixed size.
    pub fn pack(buffer: &[u8]) -> u128 {
        unimplemented!()
    }

    /// TODO(alex) 2021-02-05: Unhash the value into a packet buffer.
    pub fn unpack(buffer: &[u8]) -> &[u8] {
        unimplemented!()
    }
}
