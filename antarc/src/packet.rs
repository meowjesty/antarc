use std::{
    io::{BufRead, Cursor, IoSlice, Read, Write},
    marker::PhantomData,
    mem,
    num::{NonZeroU16, NonZeroU32, NonZeroU8},
    time::Duration,
};

use crc32fast::Hasher;

use crate::{
    read_buffer_inc, PacketInfo, PacketKind, PacketMarker, BUFFER_CAP, END_OF_PACKET_BYTES,
    PACKED_LEN, PROTOCOL_ID_BYTES,
};

/// TODO(alex) 2021-02-09: Improve terminology:
/// http://www.tcpipguide.com/free/t_MessagesPacketsFramesDatagramsandCells-2.htm
///
/// http://www.tcpipguide.com/free/t_MessageFormattingHeadersPayloadsandFooters.htm
pub(crate) struct RawPacket {
    crc32: NonZeroU32,
    buffer: Vec<u8>,
}

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
    pub(crate) connection_id: NonZeroU16,
    /// Incremented individually for each `Host`.
    pub(crate) sequence: NonZeroU32,
    /// Acks the `Packet` sent from a remote `Host` by taking its `sequence` value.
    pub(crate) ack: u32,
    /// Represents the ack bitfields to send previous acked state in a compact manner.
    /// TODO(alex): Use this.
    pub(crate) past_acks: u16,
    /// TODO(alex) 2021-01-29: I need this to differentiate between packets, having separate
    /// `Packet` structs (union) won't cover the case when the user sends an empty packet vs the
    /// protocol sending an ack-only packet, as both would have a zero-length body.
    /// ADD(alex) 2021-01-29: The above is talking about the lower level header, that is transmitted
    /// through the network, but here, I could use the type system to encode the correct state, and
    /// convert it into a `u16` only at `send` and `recv` time. Similar to how the crc32 and
    /// sentinel values are not present in this struct, the kind also doesn't have to be, I just
    /// need to handle it in a sort of just-in-time way (encode/decode).
    /// ADD(alex) 2021-02-05: The `kind` defines the packet as a connection request, or a
    /// response, maybe a data transfer, and each is handled differently, for example, the protocol
    /// will read the `body` of a connection request, and of a fragment, but it just passes it in
    /// the case of a data transfer.
    pub(crate) kind: u8,
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
    /// transmitted via the network, `Self + ProtocolId`, even though the crc32 substitutes it.
    pub(crate) const ENCODED_SIZE: usize = mem::size_of::<Self>() + mem::size_of::<u32>();

    fn encode(&self) -> Result<Vec<u8>, String> {
        let b_protocol_id = PROTOCOL_ID_BYTES;
        let b_connection_id = self.connection_id.get().to_be_bytes();
        let b_sequence = self.sequence.get().to_be_bytes();
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
        // TODO(alex): Create an error for a failed written packet.
        // This also requires us to have a more uniform size for a packet, right now it may have any
        // size, this will stop when a packet becomes a hash_packet.
        if num_written == 0 {
            return Err("Serialization error: wrote zero size.".to_string());
        }
        assert!(num_written <= (mem::size_of::<Header>() + PROTOCOL_ID_BYTES.len()));

        cursor.flush().map_err(|fail| fail.to_string())?;
        Ok(cursor.into_inner())
    }

    fn decode(cursor: &mut Cursor<&[u8]>) -> Result<Self, String> {
        let mut read_pos = 0;
        let mut read_buffer = cursor.fill_buf().map_err(|fail| fail.to_string())?;

        let connection_id = NonZeroU16::new(read_buffer_inc!(u16, read_buffer, read_pos)).unwrap();
        let sequence = NonZeroU32::new(read_buffer_inc!(u32, read_buffer, read_pos)).unwrap();
        let ack = read_buffer_inc!(u32, read_buffer, read_pos);
        let past_acks = read_buffer_inc!(u16, read_buffer, read_pos);
        let kind = read_buffer_inc!(u8, read_buffer, read_pos);

        cursor.consume(read_pos);

        let header = Header {
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

#[derive(Debug)]
pub(crate) struct Received {
    time_received: Duration,
}

/// NOTE(alex) 2021-01-28: These are packets that were received, and the user application has
/// loaded them, so they're moved into this state.
#[derive(Debug)]
pub(crate) struct Retrieved {
    time_received: Duration,
    time_retrieved: Duration,
}

/// NOTE(alex) 2021-02-06: These packets are intercepted by the protocol and handled internally,
/// they cannot be retrieved by the use. Deals with connection requests (connection handling),
/// hearbeat, fragmentation.
/// TODO(alex) 2021-02-15: This name sucks and doesn't really convey what it does.
#[derive(Debug)]
pub(crate) struct Internal {
    time_received: Duration,
    time_internal: Duration,
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
pub(crate) struct Packet<State> {
    pub(crate) header: Header,
    pub(crate) body: Vec<u8>,
    pub(crate) state: State,
}

impl Packet<Received> {
    pub(crate) fn new(header: Header, body: Vec<u8>) -> Self {
        let state = Received {
            // TODO(alex) 2021-02-05: This function requires a timing system.
            time_received: Duration::new(0, 0),
        };
        let packet = Packet {
            header,
            body,
            state,
        };

        packet
    }

    pub(crate) fn retrieved(self) -> Packet<Retrieved> {
        let state = Retrieved {
            time_received: self.state.time_received,
            // TODO(alex) 2021-02-05: This function requires a timing system.
            time_retrieved: Duration::new(0, 0),
        };
        let packet = Packet {
            header: self.header,
            body: self.body,
            state,
        };

        packet
    }

    pub(crate) fn internald(self) -> Packet<Internal> {
        let state = Internal {
            time_received: self.state.time_received,
            // TODO(alex) 2021-02-05: This function requires a timing system.
            time_internal: Duration::new(0, 0),
        };
        let packet = Packet {
            header: self.header,
            body: self.body,
            state,
        };

        packet
    }

    /// TODO(alex) 2021-02-05: Decoding only makes sense in a `Packet<Received>` context, why would
    /// I want to decode any other state of a packet?
    pub fn decode(buffer: &[u8]) -> Result<Packet<Received>, String> {
        if buffer.len() > PACKED_LEN {
            panic!(
                "Not handling packets that have a different len {:?} > PACKED_LEN {:?}",
                buffer.len(),
                PACKED_LEN
            );
        }

        let skip_crc32 = mem::size_of::<u32>();
        // NOTE(alex): New buffer that has a `ProtocolId` at the start, instead of the crc32.
        // TODO(alex): Check if this is allocating a vec, then converting it into a slice.
        let challenge_buffer = &[&PROTOCOL_ID_BYTES, &buffer[skip_crc32..]].concat();
        // NOTE(alex): Calculate crc32 and check it against the received packet.
        let mut hasher = Hasher::new();
        hasher.update(challenge_buffer);
        let expected_crc32 = hasher.finalize();

        // NOTE(alex): Deserialize `Header` and retrieve number of bytes written.
        // TODO(alex) 2021-02-05: Can't we read the crc32 from the buffer, then pass a cursor
        // (or a read_buf) to Header::deserialize, to keep the cursor position synchronized?
        let mut cursor = Cursor::new(buffer);
        let mut read_pos = 0;
        let mut read_buffer = cursor.fill_buf().map_err(|fail| fail.to_string())?;

        let received_crc32 = read_buffer_inc!(u32, read_buffer, read_pos);
        cursor.consume(read_pos);
        if expected_crc32 != received_crc32 {
            return Err(format!(
                "CRC32 error: invalid crc32, expected {:?}, but got {:?}.",
                expected_crc32, received_crc32
            ));
        }

        let header = Header::decode(&mut cursor)?;
        let pos_after_header = cursor.position() as usize;

        // NOTE(alex): Cursor skips the decoded `Header` (which does not have the same number of
        // bytes as `size_of::<Header>()`).
        let mut cursor = Cursor::new(&buffer[pos_after_header..]);
        let body_buffer_with_marker = cursor.fill_buf().map_err(|fail| fail.to_string())?;

        // NOTE(alex): Check for a valid packet end marker.
        let offset_to_marker = body_buffer_with_marker.len() - mem::size_of::<PacketMarker>();
        let (body_buffer, marker) = body_buffer_with_marker.split_at(offset_to_marker);
        if marker != END_OF_PACKET_BYTES {
            return Err("Serialization error: wrong packet marker.".to_string());
        }

        let body = body_buffer.to_vec();

        let read_pos = body_buffer.len() + marker.len();
        cursor.consume(read_pos);

        // TODO(alex) 2021-01-24: This should be an actual time, we need some form of global timing
        // system to keep track of received, sent, lost connection, heartbeat timings.
        // ADD(alex) 2021-02-05: This function requires a timing system.
        let now = 0;
        // let packet = RawPacket::new(header, body, Metadata::Received(now));
        // Ok(packet)
        unimplemented!();
    }
}

impl Packet<ToSend> {
    pub(crate) fn new(header: Header, body: Vec<u8>) -> Self {
        let state = ToSend {
            // TODO(alex) 2021-02-05: This function requires a timing system.
            time_enqueued: Duration::new(1, 0),
        };
        let packet = Packet {
            header,
            body,
            state,
        };

        packet
    }

    pub(crate) fn sent(self) -> Packet<Sent> {
        let state = Sent {
            time_enqueued: self.state.time_enqueued,
            // TODO(alex) 2021-02-05: This function requires a timing system.
            time_sent: Duration::new(0, 0),
        };
        let packet = Packet {
            header: self.header,
            body: self.body,
            state,
        };

        packet
    }

    /// TODO(alex) 2021-02-05: Encoding only makes sense in packets we want to send (preparing to
    /// send), I can't see any reason to having this function be more generic (and be used by other
    /// states). Is there a point to having this in any other state? Why would I want to encode
    /// a `Packet<Acked>`?
    fn encode(&self) -> Result<RawPacket, String> {
        if self.body.len() > BUFFER_CAP {
            return Err("Serialization error: over buffer capacity.".to_string());
        }

        let header = self.header.encode()?;
        let write_buffers = [
            // NOTE(alex): Header buffer contains the `ProtocolId` value at its start.
            IoSlice::new(&header),
            IoSlice::new(&self.body),
            IoSlice::new(&END_OF_PACKET_BYTES),
        ];

        // TODO(alex) 2021-02-05: Consider `ptr::copy_nonoverlapping` as an alternative to `Cursor`,
        //  this is a potential performance consideration, but we're not in that phase yet
        // (`memcpy`).
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
        let crc32 = NonZeroU32::new(hasher.finalize()).unwrap();

        println!("written crc32 {:?}", crc32);

        // NOTE(alex): This overwrites the Protocol Id with crc32.
        cursor.set_position(0);
        cursor
            .write(&crc32.get().to_be_bytes())
            .map_err(|fail| fail.to_string())?;

        cursor.flush().map_err(|fail| fail.to_string())?;

        let buffer = cursor.into_inner();
        assert!(buffer.len() > 0);
        assert!(buffer.len() <= PACKED_LEN);

        let raw_packet = RawPacket { crc32, buffer };

        Ok(raw_packet)
    }
}

impl Packet<Sent> {
    pub(crate) fn acked(self) -> Packet<Acked> {
        let state = Acked {
            time_enqueued: self.state.time_enqueued,
            time_sent: self.state.time_sent,
            // TODO(alex) 2021-02-05: This function requires a timing system.
            time_acked: Duration::new(0, 0),
        };
        let packet = Packet {
            header: self.header,
            body: self.body,
            state,
        };

        packet
    }
}

/// TODO(alex) 2021-01-31: This is an impl block for packets with **any** state.
/// ADD(alex) 2021-02-05: Functions defined here conflict with functions in every other state, so
/// a `fn new` here won't allow any other state to have an `fn new`, you must use the most generic.
impl<State> Packet<State> {
    /// TODO(alex) 2021-02-05: Hash the buffer to have a fixed size.
    pub fn pack(mut buffer: Vec<u8>) -> u128 {
        unimplemented!()
    }

    /// TODO(alex) 2021-02-05: Unhash the value into a packet buffer.
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
