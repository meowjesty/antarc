use super::*;

// REGION(alex): Packet `Delivery` types:
#[derive(Debug, Clone, PartialEq)]
pub struct MetaDelivery {
    pub time: Duration,
    pub address: SocketAddr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToSend {
    pub id: PacketId,
    pub meta: MetaDelivery,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sent {
    pub id: PacketId,
    /// NOTE(alex): The time to live helps dealing with `Reliable` packets. If there was no `ttl`,
    /// then packets could remain "unacked" forever.
    ///
    /// Ignored for `Unreliable` packets, as the protocol doesn't store those.
    pub ttl: Duration,
    pub meta: MetaDelivery,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Received {
    pub meta: MetaDelivery,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Acked {
    pub id: PacketId,
    pub meta: MetaDelivery,
}

impl Deliver for ToSend {}
impl Deliver for Sent {}
impl Deliver for Received {}
impl Deliver for Acked {}
