use super::*;

impl Encoder for ConnectionRequest {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = Self::PACKET_TYPE_BYTES.to_vec();
        debug_assert_eq!(packet_type_bytes.len(), size_of::<PacketType>());

        packet_type_bytes
    }
}

impl Encoder for ConnectionAccepted {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>()
        + size_of::<ConnectionId>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = Self::PACKET_TYPE_BYTES.as_ref();
        let connection_id_bytes = self.connection_id.get().to_be_bytes();

        let encoded = [packet_type_bytes, connection_id_bytes.as_ref()].concat();
        debug_assert_eq!(
            encoded.len(),
            size_of::<PacketType>() + size_of::<ConnectionId>()
        );

        encoded
    }
}

impl Encoder for DataTransfer {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>()
        + size_of::<ConnectionId>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = Self::PACKET_TYPE_BYTES.as_ref();
        let connection_id_bytes = self.connection_id.get().to_be_bytes();
        let payload = self.payload.as_slice();

        // TODO(alex) [low] 2021-08-20: Decreased number of allocations, and hopefully increased
        // performance, according to benchmarks this is the fastest way to concatenate into a vec.
        //
        // Now the question is, do I need to return `Vec<u8>`, or could I return `&[u8]`?
        // The enconding function needs this as a vec?
        let encoded = [packet_type_bytes, connection_id_bytes.as_ref(), payload].concat();
        debug_assert_eq!(
            encoded.len(),
            size_of::<PacketType>() + size_of::<ConnectionId>() + payload.len()
        );

        encoded
    }
}

impl Encoder for Fragment {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>()
        + size_of::<u8>()
        + size_of::<u8>()
        + size_of::<ConnectionId>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = Self::PACKET_TYPE_BYTES.as_ref();
        let fragment_index_bytes = self.index.to_be_bytes();
        let fragment_total_bytes = self.total.to_be_bytes();
        let connection_id_bytes = self.connection_id.get().to_be_bytes();
        let payload = self.payload.as_slice();

        let encoded = [
            packet_type_bytes,
            fragment_index_bytes.as_ref(),
            fragment_total_bytes.as_ref(),
            connection_id_bytes.as_ref(),
            payload,
        ]
        .concat();
        debug_assert_eq!(
            encoded.len(),
            size_of::<PacketType>()
                + size_of::<u8>()
                + size_of::<u8>()
                + size_of::<ConnectionId>()
                + payload.len()
        );

        encoded
    }
}

impl Encoder for Heartbeat {
    const HEADER_SIZE: usize = size_of::<ProtocolId>()
        + size_of::<Sequence>()
        + size_of::<Ack>()
        + size_of::<PacketType>()
        + size_of::<ConnectionId>();

    fn encoded(&self) -> Vec<u8> {
        let packet_type_bytes = Self::PACKET_TYPE_BYTES.as_ref();
        let connection_id_bytes = self.connection_id.get().to_be_bytes();

        let encoded = [packet_type_bytes, connection_id_bytes.as_ref()].concat();
        debug_assert_eq!(
            encoded.len(),
            size_of::<PacketType>() + size_of::<ConnectionId>()
        );

        encoded
    }
}
