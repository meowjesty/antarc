#![feature(const_panic)]
#![feature(write_all_vectored)]
#![feature(bool_to_option)]
// https://github.com/rust-lang/rust/issues/66753#issuecomment-644285006
// #![feature(const_precise_live_drops)]
#![feature(const_fn_floating_point_arithmetic)]
#![feature(duration_saturating_ops)]
#![feature(duration_consts_2)]

use core::mem;
use std::{
    num::{NonZeroU16, NonZeroU32, NonZeroU8},
    time::Duration,
};

use packet::header::Header;

pub mod client;
pub mod host;
pub mod net;
pub mod packet;
pub mod receiver;
pub mod sender;
pub mod server;

#[macro_export]
macro_rules! read_buffer_inc {
    ({ $buffer: expr, $start: expr } : $kind: ident) => {{
        let end = $start + mem::size_of::<$kind>();
        let bytes_arr: &[u8; mem::size_of::<$kind>()] = $buffer[$start..end].try_into().unwrap();
        let val = $kind::from_be_bytes(*bytes_arr);
        $start = end;
        val
    }};
}

pub type AntarcResult<T> = Result<T, String>;

/// An exponential moving average - EMA is a type of moving average that places a greater weight and
/// significance on the most recent data points.
///
/// The weighting for each **older** datum **decreases** exponentially, never reaching zero.
pub(crate) const fn exponential_moving_average(
    new_value: Duration,
    old_value: Duration,
) -> Duration {
    let weight = 0.1;
    // TODO(alex): `const fn` complains without `saturating_add`, maybe `+` operator is not marked
    // as const fn for `Duration`?
    let result = new_value
        .mul_f64(weight)
        .saturating_add(old_value.mul_f64(1.0 - weight));
    result
}

// TODO(alex) 2021-02-27 There needs to be a check on the send queue, so that we don't keep sending
// and acking packets of the same host/peer, otherwise we might end up in a situation where other
// peers never get a message back from the server.

/// Protocol id type alias that identifies a packet as part of the protocol.
/// This won't be sent to a remote host, but it's used in the CRC32 calculation, it acts as a check
/// between hosts that the CRC32 is correct, as the receiver will take the CRC32 out of the packet,
/// insert the `ProtocolId`, calculate the packet CRC32 and check against what they received.
pub(crate) type ProtocolId = NonZeroU32;

pub(crate) type PacketMarker = NonZeroU16;

pub(crate) const PROTOCOL_ID: ProtocolId = unsafe { NonZeroU32::new_unchecked(0xbabedad) };
pub(crate) const PROTOCOL_ID_BYTES: [u8; mem::size_of::<ProtocolId>()] =
    PROTOCOL_ID.get().to_be_bytes();
pub(crate) const BUFFER_CAP: usize = Header::ENCODED_SIZE + 128;
pub(crate) const PADDING: NonZeroU8 = unsafe { NonZeroU8::new_unchecked(0xe0) };
/// Marks the end of useful data, anything past this can be skipped.
pub(crate) const END_OF_PACKET: NonZeroU16 = unsafe { NonZeroU16::new_unchecked(0x31f6) };
pub(crate) const END_OF_PACKET_BYTES: [u8; mem::size_of::<PacketMarker>()] =
    END_OF_PACKET.get().to_be_bytes();
// pub const END_OF_PACKET: PacketMarker = 0xa43a;
// pub const END_OF_PACKET_BYTES: [u8; mem::size_of::<PacketMarker>()] =
// END_OF_PACKET.to_be_bytes();
/// The whole buffer + `MARKER` + `END_OF_PACKET` markers.
pub(crate) const PACKED_LEN: usize = BUFFER_CAP + mem::size_of::<PacketMarker>();
pub(crate) const MTU_LENGTH: usize = 1500;

// TODO(alex) 2021-01-24: How does send / receive works?
// - Packet is sent with data / client replies with either data or just ack;
// - Server receives the ack, but has no data to send, then send a heartbeat to test the connection;
// TODO(alex) 2021-01-24: Add some form of expiration for a packet (stop trying to resend it, and
// remove it from the list of `to_send` packets).

// /// First type of packet sent from a client to a server.
// /// `Client -> Server`
// pub fn connection_request() -> Self {
//     let header = Header {
//         kind: NON_FRAGMENTED | CONNECTION_REQUEST,
//         ..Header::default()
//     };
//     let body = Vec::new();

//     Packet::new(header, body)
// }

// /// First type of packet sent from a server to a client.
// /// `Server -> Client`
// pub fn challenge_request(to_ack: PacketInfo, from: SocketAddr) -> Self {
//     let header = Header {
//         kind: NON_FRAGMENTED | CHALLENGE_REQUEST,
//         ack: to_ack,
//         ..Header::default()
//     };
//     let body = Vec::new();

//     Packet::new(header, body)
// }

// /// Sent back to the server to estabilish the connection.
// /// `Client -> Server`
// pub fn challenge_response(challenge_req: Header) -> Self {
//     let header = Header {
//         ack: challenge_req.sequence,
//         kind: NON_FRAGMENTED | CHALLENGE_RESPONSE,
//         ..Header::default()
//     };
//     let body = Vec::new();

//     Packet::new(header, body)
// }
