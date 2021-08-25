use antarc_protocol::{events::*, packets::raw::*, server::*, *};

use crate::*;

impl AntarcNet<Server> {
    pub fn new_server(address: SocketAddr, capacity: usize, reliable_ttl: Duration) -> Self {
        let antarc = Protocol::new_server(capacity, reliable_ttl);
        let socket = UdpSocket::bind(address).unwrap();
        let recv_buffer = vec![0; 1500 * 5];

        Self {
            antarc,
            address,
            socket,
            recv_buffer,
        }
    }

    fn poll_retry_connection_accepted(&mut self) {
        if let Some(reliable_packet) = self.antarc.retry_reliable_connection_accepted() {
            debug!("Server: ready to re-send {:#?}", reliable_packet);

            // NOTE(alex): Udp send.
            {
                let raw_packet = reliable_packet.as_raw();
                self.socket
                    .send_to(&raw_packet.bytes, raw_packet.address)
                    .unwrap();
            }

            self.antarc.sent_connection_accepted(reliable_packet);
        }
    }

    fn poll_connection_accepted(&mut self) {
        for scheduled in self
            .antarc
            .service
            .drain_connection_accepted(..)
            .collect::<Vec<_>>()
        {
            debug!("Server: preparing to send {:#?}.", scheduled);
            let packet = self.antarc.create_connection_accepted(scheduled);
            debug!("Server: ready to send {:#?}", packet);

            // NOTE(alex): Udp send.
            {
                let raw_packet = packet.as_raw();
                self.socket
                    .send_to(&raw_packet.bytes, raw_packet.address)
                    .unwrap();
            }

            self.antarc.sent_connection_accepted(packet);
        }
    }

    pub fn poll(&mut self) -> std::vec::Drain<ProtocolEvent<ServerEvent>> {
        debug!("Server: dummy poll");

        self.poll_retry_connection_accepted();
        self.poll_retry_data_transfer();
        self.poll_retry_fragment();
        self.poll_connection_accepted();
        self.poll_unreliable_data_transfer();
        self.poll_reliable_data_transfer();
        self.poll_unreliable_fragment();
        self.poll_reliable_fragment();
        self.poll_unreliable_heartbeat();
        self.poll_reliable_heartbeat();

        // NOTE(alex): UDP receive.
        let received = self.socket.recv_from(&mut self.recv_buffer);
        match received {
            Ok((bytes_len, from_address)) => {
                let raw_packet =
                    RawPacket::new(from_address, self.recv_buffer[..bytes_len].to_vec());

                if let Err(fail) = self.antarc.on_received(raw_packet) {
                    error!("Server: encountered error on received {:#?}.", fail);
                    self.antarc.service.api.push(ProtocolEvent::Fail(fail));
                }
            }
            Err(_) => todo!(),
        }

        self.antarc.poll()
    }
}
