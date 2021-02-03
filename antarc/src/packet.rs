use std::{
    io::{BufRead, Cursor, IoSlice, Read, Write},
    marker::PhantomData,
    mem,
    num::{NonZeroU16, NonZeroU32},
    time::Duration,
};

use crc32fast::Hasher;

use crate::{
    read_buffer_inc, PacketInfo, PacketKind, PacketMarker, BUFFER_CAP, END_OF_PACKET_BYTES,
    PACKED_LEN, PROTOCOL_ID_BYTES, SENT,
};

/// NOTE(alex): Valid `Packet` state transitions:
/// - Received -> Retrieved;
/// - ToSend -> Sent -> Acked;

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
#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub(crate) struct Header {
    /// WARNING(alex): This must always be the first bytes of the `Header` when converting it
    /// into (and from) buffers.
    /// TODO(alex) 2021-02-03: How to initialize this? I want a non-default value, but this won't
    /// be calculated until the `Header` is encoded. If the crc32 is not stored in the `Header`,
    /// it'll be lost, the same issue happens when moving it into the `Packet`? Maybe this is stored
    /// separately, in the packet's meta structures?
    ///
    /// This is also part of a bigger question, where do we store the encoded packets (do I even
    /// want to store them?)? If stored, then the crc32 may simply be coupled there like
    /// `(crc32, Vec<u8>)`, but if not, then it would be something similar `(crc32, Packet<Sent>)`.
    /// It seems like the place for the crc32 is not in here.
    // pub(crate) crc32: NonZeroU32,

    /// Identifies the connection, this enables network switching from either side without having
    /// to slowly re-estabilish the connection.
    pub(crate) connection_id: NonZeroU32,
    /// Incremented individually for each `Host`.
    pub(crate) sequence: NonZeroU32,
    /// Acks the `Packet` sent from a remote `Host` by taking its `sequence` value.
    pub(crate) ack: PacketInfo,
    /// Represents the ack bitfields to send previous acked state in a compact manner.
    /// TODO(alex): Use this.
    pub(crate) past_acks: PacketInfo,
    /// TODO(alex) 2021-01-29: I need this to differentiate between packets, having separate
    /// `Packet` structs (union) won't cover the case when the user sends an empty packet vs the
    /// protocol sending an ack-only packet, as both would have a zero-length body.
    /// ADD(alex) 2021-01-29: The above is talking about the lower level header, that is transmitted
    /// through the network, but here, I could use the type system to encode the correct state, and
    /// convert it into the an `u16` only at `send` and `recv` time. Similar to how the crc32 and
    /// sentinel values are not present in this struct, the kind also doesn't have to be, I just
    /// need to handle it in a sort of just-in-time way (encode/decode).
    pub(crate) kind: NonZeroU16,
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
    /// NOTE(alex): This is the size of the `Header` for the fields that are **encoded** and
    /// transmitted via the network.
    pub(crate) const ENCODED_SIZE: usize =
        mem::size_of::<PacketInfo>() * 5 + mem::size_of::<PacketKind>();

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let b_protocol_id = PROTOCOL_ID_BYTES;
        let b_connection_id = self.connection_id.to_be_bytes();
        let b_sequence = self.sequence.to_be_bytes();
        let b_ack = self.ack.to_be_bytes();
        let b_past_acks = self.past_acks.to_be_bytes();
        let b_kind = self.kind.to_be_bytes();

        // NOTE(alex): Protocol Id will be overwritten and discarded after crc32 is calculated.
        let write_buffers = [
            IoSlice::new(&b_protocol_id),
            IoSlice::new(&b_connection_id),
            IoSlice::new(&b_sequence),
            IoSlice::new(&b_ack),
            IoSlice::new(&b_past_acks),
            IoSlice::new(&b_kind),
        ];
        let mut cursor = Cursor::new(Vec::with_capacity(mem::size_of::<Header>()));
        // NOTE(alex): Move cursor to after crc32 position.
        let num_written = cursor
            .write_vectored(&write_buffers)
            .map_err(|fail| fail.to_string())?;
        // TODO(alex): Create an error for basically a failed written packet.
        // This also requires us to have a more uniform size for a packet, right now it may have any
        // size, this will stop when a packet becomes a hash_packet.
        if num_written == 0 {
            return Err("Serialization error: wrote zero size.".to_string());
        }
        assert!(num_written <= (mem::size_of::<Header>() + PROTOCOL_ID_BYTES.len()));

        cursor.flush().map_err(|fail| fail.to_string())?;
        Ok(cursor.into_inner())
    }

    pub fn decode(cursor: &mut Cursor<&[u8]>) -> Result<Self, String> {
        let mut read_pos = 0;
        let mut read_buffer = cursor.fill_buf().map_err(|fail| fail.to_string())?;

        // let crc32 = read_buffer_inc!(PacketInfo, read_buffer, read_pos);
        let connection_id = read_buffer_inc!(PacketInfo, read_buffer, read_pos);
        let sequence = read_buffer_inc!(PacketInfo, read_buffer, read_pos);
        let ack = read_buffer_inc!(PacketInfo, read_buffer, read_pos);
        let past_acks = read_buffer_inc!(PacketInfo, read_buffer, read_pos);
        let kind = read_buffer_inc!(PacketKind, read_buffer, read_pos);

        cursor.consume(read_pos);

        let header = Header {
            // crc32,
            connection_id,
            sequence,
            ack,
            past_acks,
            kind,
        };

        Ok(header)
    }
}

/// TODO(alex) 2021-01-30: This ends up being almost an exact copy of the `RawHeader`, the fields it
/// has that the raw version doesn't are:
/// - `crc32` which the raw version doesn't hold, it only gets inserted into the buffer;
/// - `protocol_id` same as above;
/// - `kind` which here is represented by an actual `Packet` type struct, such as `DataTransfer`,
/// or `Fragment`.
///
/// Should I have different packets like this? It'll make coding hard as the protocol will have
/// structs like `Acked(DataTransfer)`, `Acked(ConnectionRequest)`, `Acked(Fragment)`.
///
/// Thinking about redundant data, the packet state could be representable by composing the inner
/// data of each state transition, with the new fields required by the latter state, such as
/// the `ToSend` struct having an `enqueued_time` -> `Sent` has all the `ToSend` fields, plus
/// `time_sent` -> finally `Acked` has all of the previous, and adds `time_acked`. These compose
/// nicely, but we still have the issue of field duplication, unless each state has a field
/// containing the previous state, which seems counterintuitive, as the biggest benefit I'm seeking
/// is to have the most help possible from the compiler to prevent incorrect states from being
/// representable.
// struct Header {}

trait MetaPacket<Kind> {
    fn metadata(&self) -> Kind;
}

#[derive(Debug)]
pub(crate) struct Received {
    time_received: Duration,
}

/// TODO(alex) 2021-01-28: These are packets that were received, and the user application has
/// loaded, so they're moved into this state.
#[derive(Debug)]
pub(crate) struct Retrieved {
    time_received: Duration,
    time_retrieved: Duration,
}

#[derive(Debug)]
pub(crate) struct Sent {
    time_enqueued: Duration,
    time_sent: Duration,
}

#[derive(Debug)]
pub(crate) struct ToSend {
    time_enqueued: Duration,
}

#[derive(Debug)]
pub(crate) struct Acked {
    time_enqueued: Duration,
    time_sent: Duration,
    time_acked: Duration,
}

/// TODO(alex) 2021-02-01: By using the `State` type parameter, it becomes possible to store
/// whatever struct metadata we want here. Each packet state will hold a different `state` data.
#[derive(Debug)]
pub(crate) struct Packet<State> {
    header: Header,
    body: Vec<u8>,
    state: State,
}

/// TODO(alex) 2021-01-31: This is an impl block only for packets with `Sent` as the state.
impl Packet<Sent> {
    fn ack_packet(self) -> Packet<Acked> {
        let acked = Acked {
            time_enqueued: Duration::new(1, 0),
            time_sent: Duration::new(2, 0),
            time_acked: Duration::new(3, 0),
        };
        let packet = Packet {
            header: self.header,
            body: self.body,
            state: acked,
        };

        packet
    }
}

/// TODO(alex) 2021-01-31: This is an impl block for packets with **any** state.
impl<Kind> Packet<Kind> {
    pub fn new(header: Header, body: Vec<u8>) -> Self {
        // TODO(alex) 2021-02-01: How to deal with the state here? There must be some initial
        // default state, or this function doesn't exist, prohibiting the creation of a default
        // packet. This `new` here would be basically equal to a `impl Default`.
        Packet {
            header,
            body,
            state: todo!(),
        }
    }

    // TODO(alex): Deep into the future, try to use `ptr::copy_nonoverlapping` instead of `Cursor`,
    // it's use of memcpy will probably be faster.
    pub fn serialize(&self) -> Result<Vec<u8>, String> {
        if self.body.len() > BUFFER_CAP {
            return Err("Serialization error: over buffer capacity.".to_string());
        }

        let header = self.header.encode()?;
        let b_end_of_packet = END_OF_PACKET_BYTES;
        let write_buffers = [
            // NOTE(alex): Header buffer contains the `ProtocolId` value at its start.
            IoSlice::new(&header),
            IoSlice::new(&self.body),
            IoSlice::new(&b_end_of_packet),
        ];

        let mut cursor = Cursor::new(Vec::with_capacity(PACKED_LEN));
        let num_written = cursor
            .write_vectored(&write_buffers)
            .map_err(|fail| fail.to_string())?;
        assert!(num_written > 0);
        assert!(num_written < PACKED_LEN);
        if num_written == 0 {
            return Err("Serialization error: wrote zero size.".to_string());
        } else if num_written > PACKED_LEN - mem::size_of::<PacketMarker>() {
            panic!(
                "No space left for writing the end packet marker {:?}!",
                num_written
            );
        }

        // NOTE(alex): Calculate crc32 and add it into the packet.
        let mut hasher = Hasher::new();
        hasher.update(cursor.get_ref());
        let crc32 = hasher.finalize();

        println!("written crc32 {:?}", crc32);

        // NOTE(alex): This overwrites the Protocol Id with crc32.
        cursor.set_position(0);
        cursor
            .write(&crc32.to_be_bytes())
            .map_err(|fail| fail.to_string())?;

        cursor.flush().map_err(|fail| fail.to_string())?;

        let packet = cursor.into_inner();
        assert!(packet.len() > 0);
        assert!(packet.len() <= PACKED_LEN);
        // Ok((packet, crc32))
        Ok(packet)
    }

    // TODO(alex):  Can't we read the crc32 from the buffer, then pass a cursor (or a read_buf) to
    // Header::deserialize, to keep the cursor position synchronized?
    pub fn deserialize(buffer: &[u8]) -> Result<Self, String> {
        if buffer.len() > PACKED_LEN {
            panic!(
                "Not handling packets that have a different len {:?} > PACKED_LEN {:?}",
                buffer.len(),
                PACKED_LEN
            );
        }

        let skip_crc32 = mem::size_of::<PacketInfo>();

        // NOTE(alex): Deserialize `Header` and retrieve number of bytes written.
        let mut cursor = Cursor::new(buffer);
        let mut read_pos = 0;
        let mut read_buffer = cursor.fill_buf().map_err(|fail| fail.to_string())?;

        let read_crc32 = read_buffer_inc!(PacketInfo, read_buffer, read_pos);
        cursor.consume(read_pos);

        let header = Header::decode(&mut cursor)?;
        let pos_after_header = cursor.position() as usize;

        // NOTE(alex): New buffer that has a `ProtocolId` at the start, instead of the crc32.
        // TODO(alex): Check if this is allocating a vec, then converting it into a slice.
        let buffer = &[&PROTOCOL_ID_BYTES, &buffer[skip_crc32..]].concat();

        // NOTE(alex): Calculate crc32 and check it against the received packet.
        let mut hasher = Hasher::new();
        hasher.update(buffer);
        let expected_crc32 = hasher.finalize();

        if expected_crc32 != read_crc32 {
            return Err(format!(
                "CRC32 error: invalid crc32, expected {:?}, but got {:?}.",
                expected_crc32, read_crc32
            ));
        }

        // NOTE(alex): Cursor skips the deserialized `Header` (which might not have the same number of
        // bytes as `mem::size_of::<Header>()`).
        let mut cursor = Cursor::new(&buffer[pos_after_header..]);
        let read_buffer = cursor.fill_buf().map_err(|fail| fail.to_string())?;

        // NOTE(alex): Check for a valid packet end marker.
        let offset_to_marker = read_buffer.len() - mem::size_of::<PacketMarker>();
        let (data_buf, marker) = read_buffer.split_at(offset_to_marker);
        if marker != END_OF_PACKET_BYTES {
            return Err("Serialization error: wrong packet marker.".to_string());
        }

        let body = data_buf.to_vec();

        let read_pos = data_buf.len() + marker.len();
        cursor.consume(read_pos);

        // TODO(alex) 2021-01-24: This should be an actual time, we need some form of global timing
        // system to keep track of received, sent, lost connection, heartbeat timings.
        let now = 0;
        // let packet = RawPacket::new(header, body, Metadata::Received(now));
        // Ok(packet)
        unimplemented!();
    }

    // TODO(alex): Hash the buffer to have a fixed size.
    pub fn pack(mut buffer: Vec<u8>) -> u128 {
        unimplemented!()
    }

    // TODO(alex): Unhash the value into a packet buffer.
    pub fn unpack(buffer: &[u8]) -> &[u8] {
        unimplemented!()
    }
}

/// TODO(alex) 2021-01-30: This has to be done for each different packet state. The same pattern
/// will also appear on every type-driven state to represent changes.
/// ADD(alex) 2021-01-30: Taking a look at rust's `NonZero` implementation, I think a macro will be
/// handy to do this for every kind of possible state change.
/// `Sent` would have a from: `From<Acked>`, `From<Received>`, `From<Retrieved>`, and so on, but
/// this doesn't actually make sense, as I don't want a `Sent` packet to turn into `Received` state,
/// it shouldn't compile. The macro must take the type, plus the list of **possible** state changes,
/// and not do it for every state type. Again, transitions that make no sense, should not be
/// representable in code.
fn blah() {}
// impl From<Acked> for Sent {
//     fn from(acked: Acked) -> Self {
//         let Acked(acked) = acked;
//         Self(acked)
//     }
// }
