use std::time::Duration;

use super::{header::Header, sent::Sent, Packet, Payload, RawPacket};
use crate::AntarcResult;

#[derive(Debug)]
pub(crate) struct ToSend {
    pub(crate) time_enqueued: Duration,
}

impl Packet<ToSend> {
    pub(crate) fn new(header: Header, payload: Payload, time_enqueued: Duration) -> Self {
        let state = ToSend { time_enqueued };
        let packet = Packet {
            header,
            payload,
            state,
            footer: None,
        };

        packet
    }

    pub(crate) fn sent(self, time_sent: Duration) -> Packet<Sent> {
        let state = Sent {
            time_enqueued: self.state.time_enqueued,
            time_sent,
        };
        let packet = self.into_new_state(state);
        packet
    }

    /// TODO(alex) 2021-02-05: Encoding only makes sense in packets we want to send (preparing to
    /// send), I can't see any reason to having this function be more generic (and be used by other
    /// states). Is there a point to having this in any other state? Why would I want to encode
    /// a `Packet<Acked>`?
    pub(crate) fn encode(&self) -> AntarcResult<RawPacket> {
        todo!()
    }
}
