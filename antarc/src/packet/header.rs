use core::mem;
use std::{convert::TryInto, io::Cursor};

use super::{
    payload::Payload, Ack, Sequence, StatusCode, CONNECTION_ACCEPTED, CONNECTION_REQUEST,
    DATA_TRANSFER,
};
use crate::{AntarcResult, ProtocolId};

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
pub(crate) struct HeaderInfo {
    pub(crate) sequence: Sequence,
    pub(crate) ack: Ack,
    pub(crate) past_acks: u16,
    pub(crate) status_code: StatusCode,
    pub(crate) payload_length: u16,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub(crate) struct ConnectionRequest;

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub(crate) struct ConnectionAccepted;

#[derive(Debug, PartialEq, Clone, PartialOrd)]
pub(crate) struct DataTransfer {
    pub(crate) payload: Payload,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub(crate) struct Heartbeat;

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub(crate) struct Generic;

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub(crate) struct Header<Kind> {
    pub(crate) info: HeaderInfo,
    pub(crate) kind: Kind,
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
    pub(crate) fn connection_request() -> Header<ConnectionRequest> {
        let info = HeaderInfo {
            sequence: unsafe { Sequence::new_unchecked(1) },
            ack: 0,
            past_acks: 0,
            status_code: CONNECTION_REQUEST,
            payload_length: 0,
        };
        let kind = ConnectionRequest;

        Self { info, kind }
    }
}

impl Header<ConnectionAccepted> {
    pub(crate) fn connection_accepted(ack: u32) -> Header<ConnectionAccepted> {
        let info = HeaderInfo {
            sequence: unsafe { Sequence::new_unchecked(1) },
            ack,
            past_acks: 0,
            status_code: CONNECTION_ACCEPTED,
            payload_length: 0,
        };
        let kind = ConnectionAccepted;

        Self { info, kind }
    }
}

impl Header<DataTransfer> {
    pub(crate) fn data_transfer(
        sequence: Sequence,
        ack: u32,
        payload: Payload,
    ) -> Header<DataTransfer> {
        let payload_length = payload.len();
        let info = HeaderInfo {
            sequence,
            ack,
            past_acks: 0,
            status_code: DATA_TRANSFER,
            payload_length: payload_length.try_into().unwrap(),
        };
        let kind = DataTransfer { payload };

        Self { info, kind }
    }
}

impl Header<Heartbeat> {
    pub(crate) fn heartbeat(sequence: Sequence, ack: u32) -> Header<Heartbeat> {
        let info = HeaderInfo {
            sequence,
            ack,
            past_acks: 0,
            status_code: DATA_TRANSFER,
            payload_length: 0,
        };
        let kind = Heartbeat;

        Self { info, kind }
    }
}

pub(crate) const ENCODED_SIZE: usize = mem::size_of::<Sequence>()
    + mem::size_of::<Ack>()
    + mem::size_of::<u16>()
    + mem::size_of::<StatusCode>()
    + mem::size_of::<u16>()
    + mem::size_of::<ProtocolId>();

impl<T> Header<T> {
    // pub(crate) const PROTOCOL_ID: ProtocolId = crate::PROTOCOL_ID;

    /// NOTE(alex): This is the size of the `Header` for the fields that are **encoded** and
    /// transmitted via the network, `Self + ProtocolId`, even though the crc32 substitutes it.
    ///
    /// WARNING(alex): `size_of::<Self>` is a no-no, as it changes based on struct alignment, so
    /// these values must be calculated separately. Tuples also change alignment, so these must be
    /// added individually.

    /// public APIs that call this function, like we have for the `Host<State>`. `Header::encode`
    /// is private, meanwhile `Header::connection_request` is `pub(crate)`, for example.
    fn encode(&self) -> AntarcResult<Vec<u8>> {
        todo!()
    }

    /// TODO(alex) 2021-02-26: This will decode based on the `Header<Kind>` so we need different
    /// public APIs that call this function, like we have for the `Host<State>`. `Header::decode`
    /// is private, meanwhile `Header::connection_request` is `pub(crate)`, for example.
    fn decode(cursor: &mut Cursor<&[u8]>) -> AntarcResult<Self> {
        todo!()
    }
}
