use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct Acked {
    pub time: Duration,
}
