use core::mem;
use std::io::Cursor;

use super::{Ack, Sequence, StatusCode};
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
pub(crate) struct Header {
    /// TODO(alex) 2021-02-05: The `kind` defines the packet as a connection request, or a
    /// response, maybe a data transfer, and each is handled differently, for example, the protocol
    /// will read the `body` of a connection request, and of a fragment, but it just passes it in
    /// the case of a data transfer.
    /// ADD(alex) 2021-02-28: Borrow this from http's status code, so `100 to 199` indicates a
    /// connection request of some sort (fragment, not-fragmented, first attempt, not first
    /// attempt, this is a reconnection, ...), same for other codes.

    /// WARNING(alex): This is **NOT** sent, it's used only during `encoding/decoding` to check the
    /// `Packet` validity.
    // pub(crate) protocol_id: ProtocolId,

    /// Acks the `Packet` sent from a remote `Host` by taking its `sequence` value.
    pub(crate) ack: Ack,
    /// Represents the ack bitfields to send previous acked state in a compact manner.
    /// TODO(alex): Use this.
    pub(crate) past_acks: u16,
    /// TODO(alex) 2021-04-02: This is a bit redundant, whatever is using `Header` should always
    /// know which kind it is, but the value itself still needs to be sent over the network, so
    /// I'm keeping this in here (for now), if it's ever taken out, remember to update
    /// the `Header::ENCODED_SIZE` to account for the change.
    ///
    /// ADD(alex) 2021-04-02: The way it's working right now is:
    /// - `0b0001` : rightmost bit indicates presence of connection id (`0` is ausence);
    /// - `0b0010` : right bit indicates packet origin (`1` for server, `0` for client);
    pub(crate) status_code: StatusCode,
    pub(crate) payload_length: u16,
}

/// TODO(alex) 2021-01-31: I don't want methods in the `Header`, the `kind` will be defined by the
/// `impl Packet<Kind>` of each packet type, but this will have a ton of code duplication, as the
/// only difference will be the `Header.kind` field. Should these be macros, that create
/// `impl Header<Kind>` for each kind of packet as well?
///
/// ADD(alex): 2021-02-03: Code duplication will be a minor issue with the state approach I'm using,
/// just have an `impl` block for the relevant meta-state part of each struct that feeds the
/// `Header.kind` field during encoding / decoding.
impl Header {
    // pub(crate) const PROTOCOL_ID: ProtocolId = crate::PROTOCOL_ID;

    /// NOTE(alex): This is the size of the `Header` for the fields that are **encoded** and
    /// transmitted via the network, `Self + ProtocolId`, even though the crc32 substitutes it.
    ///
    /// WARNING(alex): `size_of::<Self>` is a no-no, as it changes based on struct alignment, so
    /// these values must be calculated separately. Tuples also change alignment, so these must be
    /// added individually.
    pub(crate) const ENCODED_SIZE: usize = mem::size_of::<Sequence>()
        + mem::size_of::<Ack>()
        + mem::size_of::<u16>()
        + mem::size_of::<StatusCode>()
        + mem::size_of::<u16>()
        + mem::size_of::<ProtocolId>();

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

impl Default for Header {
    fn default() -> Self {
        Self {
            ack: 0,
            past_acks: 0b0,
            status_code: 0x0,
            payload_length: 0,
        }
    }
}
