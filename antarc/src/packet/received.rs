use std::time::Duration;

use super::{header::Header, Footer, Internal, Packet, Payload, Retrieved};
use crate::AntarcResult;

/// TODO(alex) 2021-03-07: Add `num_recvd` to know the number of bytes that were received.
/// This will be quite common among all packets, so maybe having a `num_bytes` is more appropriate?
#[derive(Debug)]
pub(crate) struct Received {
    pub(crate) time_received: Duration,
}

impl Packet<Received> {
    pub(crate) fn new(
        header: Header,
        payload: Payload,
        footer: Option<Footer>,
        time_received: Duration,
    ) -> Self {
        let state = Received { time_received };
        let packet = Packet {
            header,
            payload,
            state,
            footer,
        };

        packet
    }

    pub(crate) fn retrieved(self, time_retrieved: Duration) -> Packet<Retrieved> {
        let state = Retrieved {
            time_received: self.state.time_received,
            time_retrieved,
        };
        let packet = self.into_new_state(state);
        packet
    }

    pub(crate) fn internald(self, time_internal: Duration) -> Packet<Internal> {
        let state = Internal {
            time_received: self.state.time_received,
            time_internal,
        };
        let packet = self.into_new_state(state);
        packet
    }

    /// TODO(alex) 2021-02-05: Decoding only makes sense in a `Packet<Received>` context, why would
    /// I want to decode any other state of a packet?
    pub fn decode(buffer: &[u8], time_received: Duration) -> AntarcResult<Packet<Received>> {
        todo!()
    }
}
