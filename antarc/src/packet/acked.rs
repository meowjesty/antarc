use std::time::Duration;

#[derive(Debug)]
pub(crate) struct Acked {
    pub(crate) time_enqueued: Duration,
    pub(crate) time_sent: Duration,
    pub(crate) time_acked: Duration,
}
