use core::{
    num::{NonZeroU16, NonZeroU32},
    time::Duration,
};
use std::{collections::HashMap, net::SocketAddr};

pub type Sequence = NonZeroU32;
pub type ConnectionId = NonZeroU16;
pub type Payload = Vec<u8>;

pub enum Header {
    ConnectionRequest {
        info: HeaderInfo,
        meta: MetaInfo,
    },
    ConnectionAccepted {
        info: HeaderInfo,
        connection_id: ConnectionId,
        meta: MetaInfo,
    },
    DataTransfer {
        info: HeaderInfo,
        connection_id: ConnectionId,
        payload_length: u32,
        meta: MetaInfo,
    },
    Fragment {
        info: HeaderInfo,
        fragment_part: u32,
        connection_id: ConnectionId,
        payload_length: u32,
        meta: MetaInfo,
    },
    Heartbeat {
        info: HeaderInfo,
        connection_id: ConnectionId,
        meta: MetaInfo,
    },
}

pub struct HeaderInfo {
    pub sequence: Sequence,
    pub ack: u32,
    pub code: u16,
    pub payload_length: u32,
}

pub struct MetaInfo {
    pub time: Duration,
}

pub enum Packet {
    Reliable {
        attempts: u32,
        header: Header,
        payload: Payload,
    },
    Unreliable {
        header: Header,
        payload: Payload,
    },
}

pub enum Host {
    RequestingConnection {},
    AwaitingConnectionAck {},
    Connected {},
    Disconnected {},
}

pub enum Antarc {
    Server { remotes: HashMap<SocketAddr, Host> },
    Client { remote: Host },
}

// TODO(alex) [vhigh] 2021-07-27: Implement decode / encode, will it require a PartialPacket?
