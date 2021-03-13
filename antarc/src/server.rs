use std::{
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
    time::{Duration, Instant},
};

use crate::{
    host::{AcceptingConnection, Connected, Disconnected, Host},
    net::NetManager,
    packet::{ConnectionId, Packet, Payload, Received},
    AntarcResult, MTU_LENGTH,
};

#[derive(Debug)]
pub struct Server {
    connection_id_tracker: ConnectionId,
    disconnected: Vec<Host<Disconnected>>,
    acking_connection: Vec<Host<AcceptingConnection>>,
    connected: Vec<Host<Connected>>,
}

impl NetManager<Server> {
    pub fn new_server(address: &SocketAddr) -> Self {
        let socket = UdpSocket::bind(address).unwrap();

        let server = Server {
            connection_id_tracker: unsafe { ConnectionId::new_unchecked(1) },
            disconnected: Vec::with_capacity(8),
            acking_connection: Vec::with_capacity(8),
            connected: Vec::with_capacity(8),
        };

        let buffer = vec![0x0; MTU_LENGTH];

        NetManager {
            socket,
            buffer,
            client_or_server: server,
        }
    }

    /// TODO(alex) 2021-03-08: We need an API like `get('/{:id}')` route, but for `Host`s.
    pub fn listen(&mut self) -> AntarcResult<()> {
        let server = &mut self.client_or_server;

        loop {
            let (size_recv, remote_addr) = self
                .socket
                .recv_from(&mut self.buffer)
                .map_err(|fail| fail.to_string())?;

            if let Some(disconnected) = server
                .disconnected
                .iter()
                .find(|host| host.address == remote_addr)
            {
            } else if let Some(connecting) = server
                .connecting
                .iter()
                .find(|host| host.address == remote_addr)
            {
            } else if let Some(connected) = server
                .connected
                .iter()
                .find(|host| host.address == remote_addr)
            {
            } else {
                let mut disconnected = Host::new(remote_addr);
                let packet =
                    Packet::<Received>::decode(&self.buffer, disconnected.timer.elapsed())?;
                disconnected.ack_tracker = packet.header.get_ack();
                disconnected.received_list.push(packet);

                let mut connecting = match disconnected
                    .ack_connection(&self.socket, server.connection_id_tracker)
                {
                    Ok(_) => {
                        let connecting = disconnected.connecting();
                        connecting
                    }
                    // TODO(alex) 2021-03-08: We end up eating the error here, this is not good for
                    // composition and recovery, but how to change host connection state on success,
                    // or keep it the same on failure in a nice composable way?
                    Err(failed) => {
                        server.disconnected.push(disconnected);
                        continue;
                    }
                };

                let packet = connecting.received_list.pop().unwrap();
                connecting
                    .internals
                    .push(packet.internald(connecting.timer.elapsed()));
                server.connection_id_tracker =
                    unsafe { ConnectionId::new_unchecked(server.connection_id_tracker.get() + 1) };
                server.connecting.push(connecting);
                continue;
            }
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
