use std::{mem::size_of, net::SocketAddr, num::NonZeroU32, time::Duration};

use crc32fast::Hasher;

use super::{
    header::{Header, HeaderInfo},
    payload::Payload,
    ConnectionId, Footer, Packet, Sent,
};
use crate::{ProtocolId, PROTOCOL_ID_BYTES};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Queued {
    pub(crate) time: Duration,
    pub(crate) destination: SocketAddr,
}

impl Packet<Queued> {
    // I don't remember if this encode was in a proper working state when the `restart` branch was
    // created.
    pub(crate) fn encode(
        payload: &Payload,
        header: &HeaderInfo,
        connection_id: Option<ConnectionId>,
    ) -> (Vec<u8>, Footer) {
        let sequence_bytes = header.sequence.get().to_be_bytes().to_vec();
        let ack_bytes = header.ack.to_be_bytes().to_vec();
        let past_acks_bytes = header.past_acks.to_be_bytes().to_vec();
        let status_code_bytes = header.status_code.to_be_bytes().to_vec();
        let payload_length_bytes = header.payload_length.to_be_bytes().to_vec();

        let mut hasher = Hasher::new();
        let mut bytes = vec![
            PROTOCOL_ID_BYTES.to_vec(),
            sequence_bytes,
            ack_bytes,
            past_acks_bytes,
            status_code_bytes,
            payload_length_bytes,
            payload.0.clone(),
        ]
        .concat();

        if let Some(connection_id) = connection_id {
            let mut connection_id_bytes = connection_id.get().to_be_bytes().to_vec();
            bytes.append(&mut connection_id_bytes);
        }

        hasher.update(&bytes);
        let crc32 = hasher.finalize();
        debug_assert!(crc32 != 0);

        bytes.append(&mut crc32.to_be_bytes().to_vec());

        let footer = Footer {
            connection_id,
            crc32: unsafe { NonZeroU32::new_unchecked(crc32) },
        };

        let packet_bytes = bytes[size_of::<ProtocolId>()..].to_vec();

        (packet_bytes, footer)
    }
}
