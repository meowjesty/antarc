use std::{
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
    time::{Duration, Instant},
};

use hecs::World;

use crate::{net::NetManager, packet::ConnectionId, AntarcResult, MTU_LENGTH};

#[derive(Debug)]
pub struct Server {
    connection_id_tracker: ConnectionId,
}

impl NetManager<Server> {
    pub fn new_server(address: &SocketAddr) -> Self {
        let socket = UdpSocket::bind(address).unwrap();

        let timer = Instant::now();

        let server = Server {
            connection_id_tracker: unsafe { ConnectionId::new_unchecked(1) },
        };

        let buffer = vec![0x0; MTU_LENGTH];

        let world = World::new();

        NetManager {
            world,
            socket,
            timer,
            buffer,
            client_or_server: server,
        }
    }

    /// TODO(alex) 2021-03-08: We need an API like `get('/{:id}')` route, but for `Host`s.
    pub fn listen(&mut self) -> () {
        todo!()
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
