use std::num::{NonZeroU16, NonZeroU32};

use self::{raw::RawPacket, scheduled::Scheduled};
use crate::{
    controls::{
        connection_accepted::ConnectionAccepted, connection_denied::ConnectionDenied,
        connection_request::ConnectionRequest, connection_terminate::ConnectionTerminate,
        data_transfer::DataTransfer, heartbeat::Heartbeat,
    },
    header::HeaderInfo,
    PacketId,
};

pub mod acked;
pub mod partial;
pub mod raw;
pub mod received;
pub mod scheduled;
pub mod sent;

/// Packets might be either:
/// - FRAGMENTED or NON_FRAGMENTED;
/// - DATA_TRANSFER or CONNECTION_REQUEST or CHALLENGE or CHALLENGE_RESPONSE;
///
/// this is valid:
/// - `FRAGMENTED | DATA_TRANSFER`
///
/// but this is NOT:
/// - `FRAGMENTED | DATA_TRANSFER | CHALLENGE`
pub type StatusCode = u16;
pub const RESERVED: StatusCode = 0;
// pub const FRAGMENTED: StatusCode = 1;
pub const CONNECTION_REQUEST: StatusCode = 100;
pub const CHALLENGE_REQUEST: StatusCode = 200;
pub const CHALLENGE_RESPONSE: StatusCode = 300;
pub const CONNECTION_ACCEPTED: StatusCode = 400;
pub const CONNECTION_DENIED: StatusCode = 500;
pub const HEARTBEAT: StatusCode = 600;
pub const DATA_TRANSFER: StatusCode = 700;
pub const ACK: StatusCode = 800;

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

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct Footer {
    pub connection_id: Option<ConnectionId>,
    pub crc32: NonZeroU32,
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

// TODO(alex) 2021-05-21: Take out payload from everything related to packet, this will become a
// dynamic-ish thing that moves with the events and/or packets as neccessary. The idea is:
// - user calls enschedule, gives the bytes to us;
// - give back an id to user related to the scheduled state + these bytes (now payload);
// - when trying to send, we encode these bytes in the middle of a new vector, which contain packet
// meta information (sequence, ack, ...);
// - on success, the packet changes state, the payload is taken out and discarded;
// - on error, the packet stays as scheduled, and the payload moves along with the error event;
// - when trying to receive, we store the payload in a tuple with the packet, this way, when the
// user calls retrieve, we just move the payload to them, and the packet changes state;
// This will get rid of ownership issues regarding the payload, avoiding Arc/Weak altogether.

// TODO(alex) 2021-05-15: Finish refactoring this.
#[derive(Debug, Clone, PartialEq)]
pub struct Packet<State, MessageType> {
    pub id: PacketId,
    pub header_info: HeaderInfo,
    pub state: State,
    pub message: MessageType,
}
