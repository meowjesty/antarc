use super::*;

// REGION(alex): Packet `Message` types:
#[derive(Debug, Clone, PartialEq)]
pub struct MetaMessage {
    /// TODO(alex) [low] 2021-08-02: This is a common field for every `Packet`, but it's also being
    /// used by the `Scheduled` types, thus if I take it out from the `MetaMessage`, it has to be
    /// inserted in both structs.
    pub packet_type: PacketType,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionRequest {
    pub meta: MetaMessage,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionAccepted {
    pub meta: MetaMessage,
    pub connection_id: ConnectionId,
}

#[derive(Clone, PartialEq)]
pub struct DataTransfer {
    pub meta: MetaMessage,
    pub connection_id: ConnectionId,
    pub payload: Arc<Payload>,
}

impl core::fmt::Debug for DataTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataTransfer")
            .field("meta", &self.meta)
            .field("connection_id", &self.connection_id)
            .field("payload (length)", &self.payload.len())
            .finish()
    }
}

/// NOTE(alex): The fragment `sequence` is used as its id, making it impossible to ack a single one.
/// In order to ack it, a group of fragments is treated as the actual packet.
#[derive(Clone, PartialEq)]
pub struct Fragment {
    pub meta: MetaMessage,
    pub connection_id: ConnectionId,
    // TODO(alex) [mid] 2021-08-19: To avoid dealing with a `fragment_id`, I'll be treating the
    // `sequence` as its id. We're treating a group of fragments as 1 single packet, so we either
    // receive the whole group and it becomes 1 fully formed packet (converting into a
    // `DataTransfer`), or we discard the parts after they pass some `ttl`.
    //
    // This means that the sequence tracker for a `Peer` only increases after the last fragment is
    // sent out.
    // pub fragment_id: u64,
    pub index: u8,
    pub total: u8,
    pub payload: Arc<Payload>,
}

impl core::fmt::Debug for Fragment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fragment")
            .field("meta", &self.meta)
            .field("connection_id", &self.connection_id)
            // .field("id", &self.fragment_id)
            .field("index", &self.index)
            .field("total", &self.total)
            .field("payload (length)", &self.payload.len())
            .finish()
    }
}

impl Fragment {
    pub(crate) fn new(
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        fragment_index: usize,
        fragment_total: usize,
    ) -> Self {
        let meta = MetaMessage {
            packet_type: Self::PACKET_TYPE,
        };
        let fragment = Self {
            meta,
            connection_id,
            index: fragment_index as u8,
            total: fragment_total as u8,
            payload,
        };

        fragment
    }
}

impl From<Vec<Packet<Received, Fragment>>> for Packet<Received, DataTransfer> {
    fn from(mut fragments: Vec<Packet<Received, Fragment>>) -> Self {
        let last_fragment = fragments.pop().expect("Fragment must exist!");

        let delivery = Received {
            meta: last_fragment.delivery.meta,
        };
        let message = DataTransfer {
            meta: MetaMessage {
                packet_type: DataTransfer::PACKET_TYPE,
            },
            connection_id: last_fragment.message.connection_id,
            payload: Arc::new(fragments.into_iter().fold(
                Vec::with_capacity(last_fragment.message.total as usize * 1500),
                |payload, fragment| {
                    vec![
                        payload,
                        Arc::try_unwrap(fragment.message.payload).expect("Only owner!"),
                    ]
                    .concat()
                },
            )),
        };

        let packet = Packet {
            delivery,
            sequence: last_fragment.sequence,
            ack: last_fragment.ack,
            message,
        };

        packet
    }
}

impl DataTransfer {
    pub(crate) fn new(connection_id: ConnectionId, payload: Arc<Payload>) -> Self {
        let meta = MetaMessage {
            packet_type: Self::PACKET_TYPE,
        };
        let data_transfer = Self {
            meta,
            connection_id,
            payload,
        };

        data_transfer
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Heartbeat {
    pub meta: MetaMessage,
    pub connection_id: ConnectionId,
}

impl Heartbeat {
    pub(crate) fn new(connection_id: ConnectionId) -> Self {
        let meta = MetaMessage {
            packet_type: Self::PACKET_TYPE,
        };
        let heartbeat = Self {
            meta,
            connection_id,
        };

        heartbeat
    }
}

impl Message for ConnectionRequest {
    // TODO(alex) [low] 2021-08-05: Figure out a way to make this incompatible between the Message
    // types. Right now these constants are still programmer enforced, the compiler will happily
    // accept any when we create a packet.
    //
    // One easy way of enforcing the correct packet types would be to use a `new()` function, but
    // I think it would still be a pretty lame solution.
    const PACKET_TYPE: PacketType = CONNECTION_REQUEST;
    const PACKET_TYPE_BYTES: [u8; 1] = Self::PACKET_TYPE.to_be_bytes();
}

impl Message for ConnectionAccepted {
    const PACKET_TYPE: PacketType = CONNECTION_ACCEPTED;
    const PACKET_TYPE_BYTES: [u8; 1] = Self::PACKET_TYPE.to_be_bytes();
}

impl Message for DataTransfer {
    const PACKET_TYPE: PacketType = DATA_TRANSFER;
    const PACKET_TYPE_BYTES: [u8; 1] = Self::PACKET_TYPE.to_be_bytes();
}

impl Message for Fragment {
    const PACKET_TYPE: PacketType = FRAGMENT;
    const PACKET_TYPE_BYTES: [u8; 1] = Self::PACKET_TYPE.to_be_bytes();
}
impl Message for Heartbeat {
    const PACKET_TYPE: PacketType = HEARTBEAT;
    const PACKET_TYPE_BYTES: [u8; 1] = Self::PACKET_TYPE.to_be_bytes();
}
