use antarc_protocol::{client::*, errors::*, events::*, packets::raw::*, *};

use crate::*;

impl AntarcNet<Client> {
    pub fn new_client(address: SocketAddr) -> Self {
        let antarc = Protocol::new_client();
        let socket = UdpSocket::bind(address).unwrap();
        let recv_buffer = vec![0; 1500 * 5];

        Self {
            antarc,
            address,
            socket,
            recv_buffer,
        }
    }

    pub fn connect(&mut self, remote_address: SocketAddr) -> Result<(), ProtocolError> {
        info!("Client: connect to {:#?}.", remote_address);
        self.antarc.connect(remote_address)
    }

    pub fn poll(&mut self) -> std::vec::Drain<ProtocolEvent<ClientEvent>> {
        info!("Client: dummy poll");

        self.poll_retry_connection_request();
        self.poll_retry_data_transfer();
        self.poll_connection_request();
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
                    error!("Client: encountered error on received {:#?}.", fail);
                    self.antarc.service.api.push(ProtocolEvent::Fail(fail));
                }
            }
            Err(_) => todo!(),
        }

        self.antarc.poll()
    }

    fn poll_connection_request(&mut self) {
        for scheduled in self
            .antarc
            .service
            .drain_connection_request(..)
            .collect::<Vec<_>>()
        {
            debug!("Client: preparing to send {:#?}.", scheduled);
            let packet = self.antarc.create_connection_request(scheduled);
            debug!("Client: ready to send {:#?}", packet);

            // NOTE(alex): Udp send.
            {
                let raw_packet = packet.as_raw();
                self.socket
                    .send_to(&raw_packet.bytes, raw_packet.address)
                    .unwrap();
            }

            self.antarc.sent_connection_request(packet);
        }
    }

    fn poll_retry_connection_request(&mut self) {
        if let Some(reliable_packet) = self.antarc.resend_reliable_connection_request() {
            debug!("Client: ready to re-send {:#?}", reliable_packet);

            // NOTE(alex): Udp send.
            {
                let raw_packet = reliable_packet.as_raw();
                self.socket
                    .send_to(&raw_packet.bytes, raw_packet.address)
                    .unwrap();
            }

            self.antarc.sent_connection_request(reliable_packet);
        }
    }
}
