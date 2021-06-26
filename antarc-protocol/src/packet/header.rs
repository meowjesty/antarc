use core::mem;
use std::{convert::TryInto, io::Cursor, marker::PhantomData};

use super::{
    payload::Payload, Ack, Sequence, StatusCode, CONNECTION_ACCEPTED, CONNECTION_REQUEST,
    DATA_TRANSFER,
};
use crate::ProtocolId;

/// ### Network Component
/// --> `Packet` entity.
/// This component contains the packet metadata  that allow sequencing and
/// acknowledgement of packets.
// TODO(alex): Now that crc32 isn't in the `Header` struct, we basically lose it after the
// deserialization check, is this desirable? Is there any reason to keep the crc32? Will we
// ever need to check against it again?
// Having it inside `Header` means that serialization takes a `&mut Packet`, so that the crc32 can
// be inserted, or the alternative where it returns a `(Vec<u8>, crc32)`, which delegates the
// insertion of crc32 back into the packet, to whoever is calling `packet.serialize()`.
///
/// ADD(alex) 2021-02-09: I've missed the concept of a `Footer` completely, putting the crc32 in it
/// avoids the dance around `protocol_id` <-> `crc32` swap.
/// http://www.tcpipguide.com/free/t_MessageFormattingHeadersPayloadsandFooters.htm
///
/// ADD(alex) 2021-02-10: It's probably also important to include the length of the data in the
/// header, to avoid dealing with _markers_ for the start/end of the payload. Having a `payload_len`
/// makes everything simpler to handle when encoding/decoding, we already know where the `Header`
/// ends (`size_of::<Header>`), with this field we get to just offset `buffer[payload_len]` to get
/// the `Footer` (with the `crc32`), keeping the `Footer` out of crc32 calculation.
///
/// ADD(alex) 2021-02-26: `kind` is a limited set of variants, so maybe `Header<Kind>` makes sense?
///
/// ADD(alex) 2021-03-01: The `Header must be set with the following order:
/// 0. protocol id (during encoding / decoding, not sent / received);
/// 1. type of header;
///   - the type of header will contain the extra data, also indicating the lack of some fields
/// 2. connection id;
/// 3. rest of the fields that are common for every header type;
/// TODO(alex) 2021-03-07: `status_code` does not represent `CONNECTION_ACCEPTED`,
/// `CONNECTION_DENIED`, it should represent `Success`, `Failed`, `Refused`, ...

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct HeaderInfo {
    pub sequence: Sequence,
    pub ack: Ack,
    pub past_acks: u16,
    pub status_code: StatusCode,
    pub payload_length: u16,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct ConnectionRequest;

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct ConnectionAccepted;

// TODO(alex) 2021-06-13 [high]: Is there any reason to keep the list of all payloads received/sent?
// When the user schedules a packet, we keep the payload as part of the event, so we ping-pong the
// payload until it's properly sent, or the packet is cancelled.
// On the receiver side, we don't care at all about the payload, as there is no failure point, we
// hold it until the user retrieves all payloads and that's it.
// Think dropping it from here makes perfect sense.
#[derive(Debug, PartialEq, Clone, PartialOrd)]
pub struct DataTransfer;

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct Heartbeat;

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct Generic;

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct Header<Kind> {
    pub info: HeaderInfo,
    pub marker: PhantomData<Kind>,
}

/// TODO(alex) 2021-01-31: I don't want methods in the `Header`, the `kind` will be defined by the
/// `impl Packet<Kind>` of each packet type, but this will have a ton of code duplication, as the
/// only difference will be the `Header.kind` field. Should these be macros, that create
/// `impl Header<Kind>` for each kind of packet as well?
///
/// ADD(alex): 2021-02-03: Code duplication will be a minor issue with the state approach I'm using,
/// just have an `impl` block for the relevant meta-state part of each struct that feeds the
/// `Header.kind` field during encoding / decoding.
impl Header<ConnectionRequest> {
    pub fn connection_request() -> Header<ConnectionRequest> {
        let info = HeaderInfo {
            sequence: unsafe { Sequence::new_unchecked(1) },
            ack: 0,
            past_acks: 0,
            status_code: CONNECTION_REQUEST,
            payload_length: 0,
        };

        Self {
            info,
            marker: PhantomData::default(),
        }
    }
}

impl Header<ConnectionAccepted> {
    pub fn connection_accepted(ack: u32) -> Header<ConnectionAccepted> {
        let info = HeaderInfo {
            sequence: unsafe { Sequence::new_unchecked(1) },
            ack,
            past_acks: 0,
            status_code: CONNECTION_ACCEPTED,
            payload_length: 0,
        };

        Self {
            info,
            marker: PhantomData::default(),
        }
    }
}

impl Header<DataTransfer> {
    pub fn data_transfer(
        sequence: Sequence,
        ack: u32,
        payload_length: u16,
    ) -> Header<DataTransfer> {
        let info = HeaderInfo {
            sequence,
            ack,
            past_acks: 0,
            status_code: DATA_TRANSFER,
            payload_length,
        };

        Self {
            info,
            marker: PhantomData::default(),
        }
    }
}

impl Header<Heartbeat> {
    pub fn heartbeat(sequence: Sequence, ack: u32) -> Header<Heartbeat> {
        let info = HeaderInfo {
            sequence,
            ack,
            past_acks: 0,
            status_code: DATA_TRANSFER,
            payload_length: 0,
        };

        Self {
            info,
            marker: PhantomData::default(),
        }
    }
}

pub const ENCODED_SIZE: usize = mem::size_of::<Sequence>()
    + mem::size_of::<Ack>()
    + mem::size_of::<u16>()
    + mem::size_of::<StatusCode>()
    + mem::size_of::<u16>()
    + mem::size_of::<ProtocolId>();
