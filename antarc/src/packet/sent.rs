use std::time::Duration;

use super::{acked::Acked, Packet};

/// TODO(alex) 2021-03-07: Add `num_sent` to know the number of bytes that were actually sent.
#[derive(Debug)]
pub(crate) struct Sent {
    pub(crate) time_enqueued: Duration,
    pub(crate) time_sent: Duration,
}

impl Packet<Sent> {
    pub(crate) fn acked(self, time_acked: Duration) -> Packet<Acked> {
        let state = Acked {
            time_enqueued: self.state.time_enqueued,
            time_sent: self.state.time_sent,
            time_acked,
        };
        let packet = self.into_new_state(state);
        packet
    }
}
