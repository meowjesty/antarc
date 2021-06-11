use std::{
    convert::TryInto,
    net::SocketAddr,
    num::NonZeroU32,
    ops::{Deref, DerefMut},
};

type PacketCode = u16;

struct Sequence(NonZeroU32);

type PayloadLength = u16;
struct Payload(Vec<u8>);

impl Payload {
    fn len(&self) -> PayloadLength {
        self.0.len().try_into().unwrap()
    }
}

impl Deref for Payload {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Payload {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

struct CommonInfo {
    sequence: Sequence,
    code: PacketCode,
}

enum Packet {
    Scheduled {
        payload: Payload,
    },
    ConnectionRequest {
        info: CommonInfo,
    },
    ConnectionAccepted {
        info: CommonInfo,
        ack: u32,
    },
    DataTransfer {
        info: CommonInfo,
        ack: u32,
        payload: Payload,
    },
    Heartbeat {
        info: CommonInfo,
        ack: u32,
    },
}

struct Host {
    address: SocketAddr,
    scheduled: Vec<Packet>,
    sent: Vec<Packet>,
    received: Vec<Packet>,
}

enum Connection {
    Disconnected { host: Host },
    Requesting { host: Host },
    Awaiting { host: Host },
    Connected { host: Host },
}
