use crate::{ack_window::AckWindow, sequence::Sequence, Message, Reliability};

pub(crate) struct Record<R: Reliability, M: Message> {
    sequence: Sequence,
    ack: Sequence,
    ack_window: AckWindow,
    message: M,
    reliability: R,
}
