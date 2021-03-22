use core::mem;
use std::io::Cursor;

use super::{Ack, ConnectionId, Sequence, StatusCode, CONNECTION_REQUEST};
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
#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct ConnectionRequestInfo {
    pub(crate) status_code: StatusCode,
    pub(crate) header_info: HeaderInfo,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct ConnectionDeniedInfo {
    pub(crate) status_code: StatusCode,
    pub(crate) header_info: HeaderInfo,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct ConnectionAcceptedInfo {
    pub(crate) status_code: StatusCode,
    pub(crate) connection_id: ConnectionId,
    pub(crate) header_info: HeaderInfo,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct DataTransferInfo {
    pub(crate) status_code: StatusCode,
    /// Identifies the connection, this enables network switching from either side without having
    /// to slowly re-estabilish the connection.
    pub(crate) connection_id: ConnectionId,
    pub(crate) header_info: HeaderInfo,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct HeaderInfo {
    /// TODO(alex) 2021-02-05: The `kind` defines the packet as a connection request, or a
    /// response, maybe a data transfer, and each is handled differently, for example, the protocol
    /// will read the `body` of a connection request, and of a fragment, but it just passes it in
    /// the case of a data transfer.
    /// ADD(alex) 2021-02-28: Borrow this from http's status code, so `100 to 199` indicates a
    /// connection request of some sort (fragment, not-fragmented, first attempt, not first
    /// attempt, this is a reconnection, ...), same for other codes.

    /// Incremented individually for each `Host`.
    pub(crate) sequence: Sequence,
    /// Acks the `Packet` sent from a remote `Host` by taking its `sequence` value.
    pub(crate) ack: Ack,
    /// Represents the ack bitfields to send previous acked state in a compact manner.
    /// TODO(alex): Use this.
    pub(crate) past_acks: u16,
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) enum Header {
    ConnectionRequest(ConnectionRequestInfo),
    ConnectionDenied(ConnectionDeniedInfo),
    ConnectionAccepted(ConnectionAcceptedInfo),
    DataTransfer(DataTransferInfo),
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
    pub(crate) const PROTOCOL_ID: ProtocolId = crate::PROTOCOL_ID;

    /// NOTE(alex): This is the size of the `Header` for the fields that are **encoded** and
    /// transmitted via the network, `Self + ProtocolId`, even though the crc32 substitutes it.
    pub(crate) const ENCODED_SIZE: usize = mem::size_of::<Self>() + mem::size_of::<ProtocolId>();

    pub(crate) const fn get_ack(&self) -> Ack {
        match self {
            Header::ConnectionRequest(request) => request.header_info.ack,
            Header::ConnectionDenied(denied) => denied.header_info.ack,
            Header::ConnectionAccepted(accepted) => accepted.header_info.ack,
            Header::DataTransfer(transfer) => transfer.header_info.ack,
        }
    }

    pub(crate) const fn get_sequence(&self) -> Sequence {
        match self {
            Header::ConnectionRequest(request) => request.header_info.sequence,
            Header::ConnectionDenied(denied) => denied.header_info.sequence,
            Header::ConnectionAccepted(accepted) => accepted.header_info.sequence,
            Header::DataTransfer(transfer) => transfer.header_info.sequence,
        }
    }

    pub(crate) const fn connection_request() -> Self {
        let header_info = HeaderInfo {
            sequence: unsafe { Sequence::new_unchecked(1) },
            ack: 0,
            past_acks: 0,
        };
        let connection_request_info = ConnectionRequestInfo {
            header_info,
            status_code: CONNECTION_REQUEST,
        };
        Header::ConnectionRequest(connection_request_info)
    }

    /// TODO(alex) 2021-02-26: This will encode based on the `Header<Kind>` so we need different
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