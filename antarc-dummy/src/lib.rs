use std::net::SocketAddr;

// TODO(alex) [mid] 2021-07-31: Create a dummy network manager implementation, that just uses
// buffers to move data, no sockets involved!
pub use antarc_protocol::{
    client::Client,
    events::AntarcEvent,
    packets::{ConnectionId, Payload},
    peers::SendTo,
    server::Server,
    Protocol,
};
use log::debug;

#[derive(Debug)]
pub struct DummyManager<Service> {
    antarc: Protocol<Service>,
    address: SocketAddr,
}

impl DummyManager<Server> {
    pub fn new_server(address: SocketAddr) -> Self {
        debug!("dummy new server");
        let antarc = Protocol::new_server();
        Self { antarc, address }
    }

    pub fn schedule(&mut self, reliable: bool, send_to: SendTo, payload: Payload) {
        debug!("dummy schedule");
        self.antarc.schedule(reliable, send_to, payload);
    }

    pub fn accept_connection(&mut self, connection_id: ConnectionId) {
        debug!("dummy accept connection");
        self.antarc.accept_connection(connection_id);
    }

    pub fn poll(&mut self) -> Vec<AntarcEvent> {
        debug!("dummy poll");

        for scheduled in self.antarc.events.scheduler.drain(..) {
            // TODO(alex) [mid] 2021-08-01: Handle the conversion of `Scheduled` into `Packet` by
            // calling some `Protocol::encode` (should be similar to the `decode`).
        }

        self.antarc.poll()
    }
}

impl DummyManager<Client> {
    pub fn new_client(address: SocketAddr) -> Self {
        let antarc = Protocol::new_client();
        Self { antarc, address }
    }

    pub fn schedule(&mut self, reliable: bool, payload: Payload) {
        debug!("dummy schedule");
        self.antarc.schedule(reliable, payload);
    }

    pub fn connect(&mut self, remote_address: SocketAddr) {
        debug!("dummy connect");
        self.antarc.connect(remote_address);
    }

    pub fn poll(&mut self) -> Vec<AntarcEvent> {
        debug!("dummy poll");

        for scheduled in self.antarc.events.scheduler.drain(..) {
            // TODO(alex) [mid] 2021-08-01: Handle the conversion of `Scheduled` into `Packet` by
            // calling some `Protocol::encode` (should be similar to the `decode`).
        }

        self.antarc.poll()
    }
}
