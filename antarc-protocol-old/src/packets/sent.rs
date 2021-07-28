use std::{net::SocketAddr, time::Duration};

#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    pub time: Duration,
    pub destination: SocketAddr,
}
