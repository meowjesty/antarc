use std::{
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
};

use crate::{
    host::{Connected, Connecting, Disconnected, Host},
    peer::NetManager,
};

#[derive(Debug)]
pub struct Server {
    disconnected: Vec<Host<Disconnected>>,
    connecting: Vec<Host<Connecting>>,
    connected: Vec<Host<Connected>>,
}

/// TODO(alex) 2021-02-14: This should be in the `net` crate, I want to avoid having sockets
/// integrated into the lower parts of the protocol. It should handle the state transitions for
/// packets, and connections, but leave the actual send/receive to the `net` crate.
impl NetManager<Server> {
    pub fn new_server(address: &SocketAddr) -> Self {
        let socket = UdpSocket::bind(address).unwrap();

        let server = Server {
            disconnected: Vec::with_capacity(8),
            connecting: Vec::with_capacity(8),
            connected: Vec::with_capacity(8),
        };

        NetManager {
            socket,
            connection_id_tracker: unsafe { NonZeroU16::new_unchecked(1) },
            kind: server,
        }
    }

    pub fn tick(&self) {
        todo!();
    }

    pub fn retrieve(&self) -> Vec<(u32, Vec<u8>)> {
        todo!();
    }

    pub fn enqueue(&self, data: Vec<u8>) {
        todo!()
    }

    pub fn ban_host(&self, host_id: u32) {
        todo!();
    }
}
