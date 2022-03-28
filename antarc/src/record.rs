use crate::{ack_window::AckWindow, length::Length, sequence::Sequence, Message, Reliability};

pub(crate) struct Record<R: Reliability, M: Message> {
    sequence: Sequence,
    ack: Sequence,
    ack_window: AckWindow,
    length: Length,
    message: M,
    reliability: R,
}
