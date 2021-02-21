use core::mem;
use packet::Header;
use std::num::{NonZeroU16, NonZeroU32, NonZeroU8};

pub mod client;
pub mod host;
mod packet;
pub mod peer;
pub mod server;

#[macro_export]
macro_rules! read_buffer_inc {
    ($kind: ident, $buffer: expr, $start: expr) => {{
        let mut bytes_arr = [0; mem::size_of::<$kind>()];
        let end = $start + mem::size_of::<$kind>();
        $buffer
            .read_exact(&mut bytes_arr)
            .map_err(|fail| fail.to_string())?;
        let val = $kind::from_be_bytes(bytes_arr);
        $start = end;
        val
    }};
}

// TODO(alex) 2021-01-25: Have separate types for `Server` and `Client`, instead of using a flag.
// Can this be an `enum Host`? `Host / Peer` distinction.
// ADD(alex): We need 2 different `send` functions:
// 1. a public `send` that only sends messages to connected hosts/peers;
// 2. an inetrnal `raw_send` that is used to actually send data, including connection requests;
// ADD(alex): There needs to be a check on the send queue, so that we don't keep sending and acking
// packets of the same host/peer, otherwise we might end up in a situation where other peers never
// get a message back from the server.

/// Protocol id type alias that identifies a packet as part of the protocol.
/// This won't be sent to a remote host, but it's used in the CRC32 calculation, it acts as a check
/// between hosts that the CRC32 is correct, as the receiver will take the CRC32 out of the packet,
/// insert the `ProtocolId`, calculate the packet CRC32 and check against what they received.
pub type ProtocolId = u32;

/// Type alias for the data which is used to implement the `Taube` protocol.
pub type PacketInfo = u32;

pub type PacketMarker = u16;

pub type TimeData = u128;

pub type PacketKind = u16;

pub const PROTOCOL_ID: NonZeroU32 = unsafe { NonZeroU32::new_unchecked(0xbabedad) };
pub const PROTOCOL_ID_BYTES: [u8; mem::size_of::<ProtocolId>()] = PROTOCOL_ID.get().to_be_bytes();
pub const BUFFER_CAP: usize = Header::ENCODED_SIZE + 128;
pub const PADDING: NonZeroU8 = unsafe { NonZeroU8::new_unchecked(0xe0) };
/// Marks the end of useful data, anything past this can be skipped.
pub const END_OF_PACKET: NonZeroU16 = unsafe { NonZeroU16::new_unchecked(0x31f6) };
pub const END_OF_PACKET_BYTES: [u8; mem::size_of::<PacketMarker>()] =
    END_OF_PACKET.get().to_be_bytes();
// pub const END_OF_PACKET: PacketMarker = 0xa43a;
// pub const END_OF_PACKET_BYTES: [u8; mem::size_of::<PacketMarker>()] =
// END_OF_PACKET.to_be_bytes();
/// The whole buffer + `MARKER` + `END_OF_PACKET` markers.
pub const PACKED_LEN: usize = BUFFER_CAP + mem::size_of::<PacketMarker>();

/// TODO(alex): 2021-02-05: How to represent these types of packets?
/// `ConnectionRequest<Packet<ToSend>>`, `ConnectionRequest<Packet<Received>>`? There'll be a bunch
/// of these structs for each type, as each outer-state may contain any inner-state:
/// `ConnectionRequest<Packet<*>>`. Could we get away with `ConnectionRequest<Packet<State>>`
/// in a generic way?
///
/// Packets might be either:
/// - FRAGMENTED or NON_FRAGMENTED;
/// - DATA_TRANSFER or CONNECTION_REQUEST or CHALLENGE or CHALLENGE_RESPONSE;
///
/// this is valid:
/// - `FRAGMENTED | DATA_TRANSFER`
///
/// but this is NOT:
/// - `FRAGMENTED | DATA_TRANSFER | CHALLENGE`
pub const SPECIAL: u8 = 0;
pub const FRAGMENTED: u8 = 1;
pub const DATA_TRANSFER: u8 = 1 << 1;
pub const CONNECTION_REQUEST: u8 = 1 << 2;
pub const CHALLENGE_REQUEST: u8 = 1 << 3;
pub const CHALLENGE_RESPONSE: u8 = 1 << 4;
pub const CONNECTION_ACCEPTED: u8 = 1 << 5;
pub const CONNECTION_DENIED: u8 = 1 << 6;
pub const HEARTBEAT: u8 = 1 << 7;

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
