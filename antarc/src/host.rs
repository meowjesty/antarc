use std::{net::SocketAddr, num::NonZeroU32};

use crate::packet::{Acked, Internal, Packet, Received, Retrieved, Sent, ToSend};

/// TODO(alex) 2021-01-29: Think of `Sessions / Channels` when wondering about connections, it helps
/// when trying to figure out how to keep alive a session (connection), how the communication
/// between hosts occur (channels trasnfer packets), and gives more struct names for similar things.
#[derive(Debug)]
pub(crate) struct Connecting {
    pub(crate) remote_addr: SocketAddr,
}

#[derive(Debug)]
pub(crate) struct Connected {
    pub(crate) remote_addr: SocketAddr,
}

#[derive(Debug)]
pub(crate) struct Disconnected {
    pub(crate) remote_addr: SocketAddr,
}

#[derive(Debug)]
pub(crate) struct Host<ConnectionState> {
    pub(crate) sequence: NonZeroU32,
    /// TODO(alex) 2021-02-13: Do not flood the network, find a way to check if the `rtt` is
    /// increasing due to us flooding the network with packets.
    pub(crate) rtt: u128,
    pub(crate) received_list: Vec<Packet<Received>>,
    pub(crate) retrieved: Vec<Packet<Retrieved>>,
    pub(crate) internals: Vec<Packet<Internal>>,
    pub(crate) send_queue: Vec<Packet<ToSend>>,
    pub(crate) sent: Vec<Packet<Sent>>,
    pub(crate) acked: Vec<Packet<Acked>>,
    pub(crate) state: ConnectionState,
}

impl Host<Disconnected> {
    /// TODO(alex) 2021-02-07: Is it possible to create a `Host` in any other state? Or should it
    /// always start in disconnected mode?
    pub(crate) fn new(remote_addr: SocketAddr) -> Host<Disconnected> {
        let state = Disconnected { remote_addr };
        let host = Host {
            sequence: unsafe { NonZeroU32::new_unchecked(1) },
            rtt: 0,
            received_list: Vec::with_capacity(32),
            retrieved: Vec::with_capacity(32),
            internals: Vec::with_capacity(32),
            send_queue: Vec::with_capacity(32),
            sent: Vec::with_capacity(32),
            acked: Vec::with_capacity(32),
            state,
        };

        host
    }

    pub(crate) fn into_connecting(self) -> Host<Connecting> {
        let state = Connecting {
            remote_addr: "127.0.0.1:7777".parse().unwrap(),
        };

        let host = Host {
            sequence: self.sequence,
            rtt: self.rtt,
            received_list: self.received_list,
            retrieved: self.retrieved,
            internals: self.internals,
            send_queue: self.send_queue,
            sent: self.sent,
            acked: self.acked,
            state,
        };

        host
    }
}
