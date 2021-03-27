use std::{
    io::{BufRead, Cursor, IoSlice, Read, Write},
    marker::PhantomData,
    mem,
    num::{NonZeroU16, NonZeroU32, NonZeroU8},
    time::Duration,
};

use crc32fast::Hasher;

use self::header::Header;
use crate::{
    read_buffer_inc, AntarcResult, PacketMarker, ProtocolId, BUFFER_CAP, END_OF_PACKET_BYTES,
    PACKED_LEN, PROTOCOL_ID_BYTES,
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
type StatusCode = u16;
pub(crate) const SPECIAL: StatusCode = 0;
pub(crate) const FRAGMENTED: StatusCode = 1;
pub(crate) const DATA_TRANSFER: StatusCode = 1 << 1;
pub(crate) const CONNECTION_REQUEST: StatusCode = 1 << 2;
pub(crate) const CHALLENGE_REQUEST: StatusCode = 1 << 3;
pub(crate) const CHALLENGE_RESPONSE: StatusCode = 1 << 4;
pub(crate) const CONNECTION_ACCEPTED: StatusCode = 1 << 5;
pub(crate) const CONNECTION_DENIED: StatusCode = 1 << 6;
pub(crate) const HEARTBEAT: StatusCode = 1 << 7;

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
pub(crate) struct ToSend {
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
    pub(crate) time_internal: Duration,
}

// REGION(alex) 2021-03-23: Types of packet (metadata).
#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct ConnectionRequest {
    host_id: u32,
    packet_id: u32,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct ConnectionDenied;

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct ConnectionAccepted {
    pub(crate) connection_id: ConnectionId,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct DataTransfer {
    /// Identifies the connection, this enables network switching from either side without having
    /// to slowly re-estabilish the connection.
    pub(crate) connection_id: ConnectionId,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct Footer {
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

/// NOTE(alex) 2021-02-01: By using the `State` type parameter, it becomes possible to store
/// whatever struct metadata we want here. Each packet state will hold a different `state` data.
#[derive(Debug)]
pub(crate) struct Packet {
    /// TODO(alex) 2021-02-28: How do we actually do this? The crc32 will only be calculated at
    /// encode time, is this `Footer` a phantasm type that will be sent at the end of the
    /// packet (when encoded), but doesn't exist as an actual type here?
    pub(crate) header: Header,
    pub(crate) payload: Payload,
    pub(crate) footer: Option<Footer>,
}

/// TODO(alex) 2021-01-31: This is an impl block for packets with **any** state.
/// ADD(alex) 2021-02-05: Functions defined here conflict with functions in every other state, so
/// a `fn new` here won't allow any other state to have an `fn new`, you must use the most generic.
impl Packet {
    pub(crate) fn encode(&self) -> Result<Vec<u8>, String> {
        todo!()
    }

    pub(crate) fn decode(buffer: &[u8]) -> Result<(Header, Payload, Footer), String> {
        todo!()
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

#[derive(Debug)]
pub(crate) struct Payload(pub(crate) Vec<u8>);
