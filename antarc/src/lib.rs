use core::mem;
use crc32fast::Hasher;
use std::{
    io::{BufRead, Cursor, IoSlice, Read, Write},
    net::SocketAddr,
    time::Instant,
};

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

#[derive(Debug)]
struct AntarcManager {
    host: Host,
    timer: Instant,
}

#[derive(Debug)]
struct Sent(Packet);

/// TODO(alex) 2021-01-30: This has to be done for each different packet state. The same pattern
/// will also appear on every type-driven state to represent changes.
/// ADD(alex) 2021-01-30: Taking a look at rust's `NonZero` implementation, I think a macro will be
/// handy to do this for every kind of possible state change.
/// `Sent` would have a from: `From<Acked>`, `From<Received>`, `From<Retrieved>`, and so on, but
/// this doesn't actually make sense, as I don't want a `Sent` packet to turn into `Received` state,
/// it shouldn't compile. The macro must take the type, plus the list of **possible** state changes,
/// and not do it for every state type. Again, transitions that make no sense, should not be
/// representable in code.
impl From<Acked> for Sent {
    fn from(acked: Acked) -> Self {
        let Acked(acked) = acked;
        Self(acked)
    }
}

#[derive(Debug)]
struct Acked(Packet);

#[derive(Debug)]
struct Received(Packet);

/// TODO(alex) 2021-01-28: These are packets that were received, and the user application has
/// loaded, so they're moved into this state.
#[derive(Debug)]
struct Retrieved(Packet);

#[derive(Debug)]
struct ToSend(Packet, SocketAddr);

#[derive(Debug)]
struct HostInfo {
    address: SocketAddr,
    rtt: u128,
    sent: Vec<Sent>,
    acked: Vec<Acked>,
    received: Vec<Received>,
}

/// TODO(alex) 2021-01-29: Think of `Sessions / Channels` when wondering about connections, it helps
/// when trying to figure out how to keep alive a session (connection), how the communication
/// between hosts occur (channels trasnfer packets), and gives more struct names for similar things.
#[derive(Debug)]
struct Connecting(HostInfo);

#[derive(Debug)]
struct Challenge(HostInfo);

#[derive(Debug)]
struct Connected(HostInfo);

#[derive(Debug)]
struct Disconnected(HostInfo);

/// NOTE(alex) 2021-01-26: The `Host`s in here are the remote hosts.
#[derive(Debug)]
struct ServerInfo {
    connecting: Vec<Connecting>,
    challenge: Vec<Challenge>,
    connected: Vec<Connected>,
    disconnected: Vec<Disconnected>,
}

#[derive(Debug)]
struct ClientInfo {
    server: HostInfo,
    connection: ConnectionState,
}

#[derive(Debug)]
enum Host {
    Client(ClientInfo),
    Server(ServerInfo),
}

#[derive(Debug)]
enum ConnectionState {
    Connecting,
    Challenged,
    Connected,
    Disconnected,
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

pub const PROTOCOL_ID: ProtocolId = 0xbabedad;
pub const PROTOCOL_ID_BYTES: [u8; mem::size_of::<ProtocolId>()] = PROTOCOL_ID.to_be_bytes();
pub const BUFFER_CAP: usize = mem::size_of::<Header>() + 128;
pub const PADDING: u8 = 0xe0;
/// Marks the end of useful data, anything past this can be skipped.
pub const END_OF_PACKET: PacketMarker = 0x31f6;
pub const END_OF_PACKET_BYTES: [u8; mem::size_of::<PacketMarker>()] = END_OF_PACKET.to_be_bytes();
// pub const END_OF_PACKET: PacketMarker = 0xa43a;
// pub const END_OF_PACKET_BYTES: [u8; mem::size_of::<PacketMarker>()] =
// END_OF_PACKET.to_be_bytes();
/// The whole buffer + `MARKER` + `END_OF_PACKET` markers.
pub const PACKED_LEN: usize = BUFFER_CAP + mem::size_of::<PacketMarker>();

/// Packets might be either:
/// - FRAGMENTED or NON_FRAGMENTED;
/// - DATA_TRANSFER or CONNECTION_REQUEST or CHALLENGE or CHALLENGE_RESPONSE;
///
/// this is valid:
/// - `FRAGMENTED | DATA_TRANSFER`
///
/// but this is NOT:
/// - `FRAGMENTED | DATA_TRANSFER | CHALLENGE`
pub const NON_FRAGMENTED: PacketKind = 1;
pub const FRAGMENTED: PacketKind = 1 << 1;
pub const DATA_TRANSFER: PacketKind = 1 << 2;
pub const CONNECTION_REQUEST: PacketKind = 1 << 3;
pub const CHALLENGE_REQUEST: PacketKind = 1 << 4;
pub const CHALLENGE_RESPONSE: PacketKind = 1 << 5;
pub const CONNECTION_ACCEPTED: PacketKind = 1 << 6;
pub const CONNECTION_DENIED: PacketKind = 1 << 7;
pub const HEARTBEAT: PacketKind = 1 << 8;

// TODO(alex) 2021-01-24: How does send / receive works?
// - Packet is sent with data / client replies with either data or just ack;
// - Server receives the ack, but has no data to send, then send a heartbeat to test the connection;
// TODO(alex) 2021-01-24: Add some form of expiration for a packet (stop trying to resend it, and
// remove it from the list of `to_send` packets).

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
pub struct Header {
    /// WARNING(alex): This must always be the first bytes of the `Header` when converting it
    /// into (and from) buffers.
    // pub crc32: PacketInfo,
    /// Identifies the connection, this enables network switching from either side without having
    /// to slowly re-estabilish the connection.
    pub connection_id: PacketInfo,
    /// Incremented individually for each `Host`.
    pub sequence: PacketInfo,
    /// Acks the `Packet` sent from a remote `Host` by taking its `sequence` value.
    pub ack: PacketInfo,
    /// Represents the ack bitfields to send previous acked state in a compact manner.
    /// TODO(alex): Use this.
    pub past_acks: PacketInfo,
    /// TODO(alex) 2021-01-29: I need this to differentiate between packets, having separate
    /// `Packet` structs (union) won't cover the case when the user sends an empty packet vs the
    /// protocol sending an ack-only packet, as both would have a zero-length body.
    /// ADD(alex) 2021-01-29: The above is talking about the lower level header, that is transmitted
    /// through the network, but here, I could use the type system to encode the correct state, and
    /// convert it into the an `u16` only at `send` and `recv` time. Similar to how the crc32 and
    /// sentinel values are not present in this struct, the kind also doesn't have to be, I just
    /// need to handle it in a sort of just-in-time way (encode/decode).
    pub kind: PacketKind,
}

impl Header {
    pub fn serialize(&self) -> Result<Vec<u8>, String> {
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

    pub fn deserialize(cursor: &mut Cursor<&[u8]>) -> Result<Self, String> {
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

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub enum Metadata {
    /// Only sent packets have this marker.
    Sent(u128),
    /// Only received packets have this marker.
    Received(u128),
    /// If a packet has this marker, it means that it was `sent + acked`.
    /// 0 -> time sent;
    /// 1 -> time acked;
    Acked(u128, u128),
    Queued(u128),
}

impl Metadata {
    pub fn time_sent(&self) -> u128 {
        match self {
            Metadata::Sent(t) => *t,
            _ => 0,
        }
    }
    pub fn time_received(&self) -> u128 {
        match self {
            Metadata::Received(t) => *t,
            _ => 0,
        }
    }
}

#[derive(Debug, PartialEq, Clone, Eq, Hash)]
pub struct Packet {
    pub header: Header,
    pub body: Vec<u8>,
    /// NOTE(alex) 2021-01-26: This is **NOT** sent with the `Packet`.
    pub metadata: Metadata,
}

impl Packet {
    pub fn new(header: Header, body: Vec<u8>, time: Metadata) -> Self {
        Packet {
            header,
            body,
            metadata: time,
        }
    }

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

    // TODO(alex): Deep into the future, try to use `ptr::copy_nonoverlapping` instead of `Cursor`,
    // it's use of memcpy will probably be faster.
    pub fn serialize(&self) -> Result<Vec<u8>, String> {
        if self.body.len() > BUFFER_CAP {
            return Err("Serialization error: over buffer capacity.".to_string());
        }

        let header = self.header.serialize()?;
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

        let header = Header::deserialize(&mut cursor)?;
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
        let packet = Packet::new(header, body, Metadata::Received(now));
        Ok(packet)
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
