use core::mem::size_of;
use std::{
    convert::{TryFrom, TryInto},
    net::SocketAddr,
    num::{NonZeroU16, NonZeroU32},
    time::{Duration, Instant},
};

use crc32fast::Hasher;
use log::{debug, error};

use self::header::Header;
use crate::{read_buffer_inc, ProtocolId, PROTOCOL_ID, PROTOCOL_ID_BYTES};

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
pub(crate) const ACK: StatusCode = 800;

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
pub type Ack = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Sequence(NonZeroU32);

impl Sequence {
    pub(crate) const fn one() -> Self {
        unsafe { Self(NonZeroU32::new_unchecked(1)) }
    }

    pub(crate) fn new(non_zero_value: u32) -> Option<Self> {
        NonZeroU32::new(non_zero_value).map(|non_zero| Sequence(non_zero))
    }

    pub(crate) unsafe fn new_unchecked(non_zero_value: u32) -> Self {
        Self(NonZeroU32::new_unchecked(non_zero_value))
    }

    pub(crate) fn get(&self) -> u32 {
        self.0.get()
    }
}

impl Default for Sequence {
    fn default() -> Self {
        unsafe { Self::new_unchecked(1) }
    }
}

#[derive(Debug, Clone, PartialEq, Default, PartialOrd)]
pub(crate) struct Payload(pub(crate) Vec<u8>);

impl Payload {
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub(crate) struct Footer {
    pub(crate) connection_id: Option<ConnectionId>,
    pub(crate) crc32: NonZeroU32,
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Queued {
    pub(crate) time: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Encoded {
    pub(crate) header: Header,
    pub(crate) bytes: Vec<u8>,
    pub(crate) footer: Footer,
    pub(crate) time: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Sent {
    pub(crate) header: Header,
    pub(crate) footer: Footer,
    pub(crate) time: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Acked {
    pub(crate) header: Header,
    pub(crate) footer: Footer,
    pub(crate) time: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Received {
    pub(crate) header: Header,
    pub(crate) footer: Footer,
    pub(crate) time: Duration,
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub(crate) enum PacketKind {
    ConnectionRequest,
    ConnectionAccepted,
    ConnectionDenied,
    Ack(Ack),
    DataTransfer,
    Heartbeat,
}

impl PacketKind {
    fn from_header(header: &Header) -> Self {
        match header.status_code {
            CONNECTION_REQUEST => PacketKind::ConnectionRequest,
            CONNECTION_DENIED => PacketKind::ConnectionDenied,
            CONNECTION_ACCEPTED => PacketKind::ConnectionAccepted,
            DATA_TRANSFER => PacketKind::DataTransfer,
            HEARTBEAT => PacketKind::Heartbeat,
            ACK => PacketKind::Ack(header.ack),
            invalid => {
                error!(
                    "Client::on_received_new_packet invalid packet type {:#?}.",
                    invalid
                );
                unreachable!();
            }
        }
    }
}

impl From<PacketKind> for StatusCode {
    fn from(kind: PacketKind) -> Self {
        match kind {
            PacketKind::ConnectionRequest => CONNECTION_REQUEST,
            PacketKind::ConnectionAccepted => CONNECTION_ACCEPTED,
            PacketKind::ConnectionDenied => CONNECTION_DENIED,
            PacketKind::Ack(_) => ACK,
            PacketKind::DataTransfer => DATA_TRANSFER,
            PacketKind::Heartbeat => HEARTBEAT,
        }
    }
}

// TODO(alex) 2021-05-15: Finish refactoring this.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Packet<State> {
    pub(crate) id: u64,
    pub(crate) payload: Payload,
    pub(crate) state: State,
    pub(crate) kind: PacketKind,
    pub(crate) address: SocketAddr,
}

impl Packet<Queued> {
    // TODO(alex) 2021-05-17: Can't do much more without implementing this function, it should take
    // priority with the `Server::tick`.
    pub(crate) fn to_encoded(
        self,
        header: Header,
        footer: Footer,
        time: &Instant,
    ) -> Packet<Encoded> {
        todo!()
    }

    // TODO(alex) 2021-05-17: Check that this code is working by comparing it with the ECS branch,
    // I don't remember if this encode was in a proper working state when the `restart` branch was
    // created.
    pub(crate) fn encode(
        &self,
        header: &Header,
        connection_id: Option<ConnectionId>,
    ) -> Result<(Vec<u8>, Footer), String> {
        let payload = &self.payload;

        let sequence_bytes = header.sequence.get().to_be_bytes().to_vec();
        let ack_bytes = header.ack.to_be_bytes().to_vec();
        let past_acks_bytes = header.past_acks.to_be_bytes().to_vec();
        let status_code_bytes = header.status_code.to_be_bytes().to_vec();
        let payload_length_bytes = header.payload_length.to_be_bytes().to_vec();

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

        Ok((packet_bytes, footer))
    }
}

impl Packet<Encoded> {
    pub(crate) fn to_sent(self, time: &Instant) -> Packet<Sent> {
        todo!()
    }
}

impl Packet<Received> {
    // TODO(alex) 2021-05-17: Check that this code is working by comparing it with the ECS branch,
    // I don't remember if this encode was in a proper working state when the `restart` branch was
    // created.
    pub(crate) fn decode(
        id: u64,
        buffer: &[u8],
        address: SocketAddr,
        timer: &Instant,
    ) -> Result<Packet<Received>, String> {
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

            let sequence = Sequence(read_sequence.try_into().unwrap());
            let header = Header {
                sequence,
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
            let connection_id = if read_status_code > CONNECTION_REQUEST {
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

            let kind = PacketKind::from_header(&header);

            let state = Received {
                header,
                footer,
                time: timer.elapsed(),
            };

            let packet = Packet {
                id,
                payload,
                state,
                address,
                kind,
            };

            Ok(packet)
        } else {
            Err(format!(
                "Received {:#?} crc32, but calculated {:#?}.",
                crc32_received, crc32
            ))
        }
    }
}
