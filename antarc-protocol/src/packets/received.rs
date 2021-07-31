use std::{net::SocketAddr, time::Duration};

#[derive(Debug, Clone, PartialEq)]
pub struct Received {
    pub time: Duration,
    pub source: SocketAddr,
}
