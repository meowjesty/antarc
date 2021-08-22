use antarc_protocol::{errors::*, events::*, packets::*, peers::*, server::*, *};

use crate::*;

impl AntarcNet<Server> {
    pub fn new_server(address: SocketAddr, capacity: usize, reliable_ttl: Duration) -> Self {
        let antarc = Protocol::new_server(capacity, reliable_ttl);
        let dummy_sender = Vec::with_capacity(100);
        let dummy_receiver = Vec::with_capacity(100);
        Self {
            antarc,
            address,
            dummy_sender,
            dummy_receiver,
        }
    }

    fn poll_retry_connection_accepted(&mut self) {
        if let Some(reliable_packet) = self.antarc.retry_reliable_connection_accepted() {
            debug!("Server: ready to re-send {:#?}", reliable_packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = reliable_packet.as_raw::<Server>();
                info!(
                    "Server: re-sent {:#?} bytes to {:#?}",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc.sent_connection_accepted(reliable_packet);
        }
    }

    fn poll_retry_data_transfer(&mut self) {
        if let Some(reliable_packet) = self.antarc.retry_reliable_data_transfer() {
            debug!("Server: ready to re-send {:#?}", reliable_packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = reliable_packet.as_raw::<Server>();
                info!(
                    "Server: re-sent {:#?} bytes to {:#?}",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc
                .sent_data_transfer(reliable_packet, ReliabilityType::Reliable);
        }
    }

    fn poll_retry_fragment(&mut self) {
        if let Some(reliable_packet) = self.antarc.retry_reliable_fragment() {
            debug!("Server: ready to re-send {:#?}", reliable_packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = reliable_packet.as_raw::<Server>();
                info!(
                    "Server: re-sent {:#?} bytes to {:#?}",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc
                .sent_fragment(reliable_packet, ReliabilityType::Reliable);
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

            // NOTE(alex): Dummy send.
            {
                let raw_packet = packet.as_raw::<Server>();
                info!(
                    "Server: sent {:#?} bytes to {:#?}",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc.sent_connection_accepted(packet);
        }
    }

    fn poll_unreliable_data_transfer(&mut self) {
        for scheduled in self
            .antarc
            .drain_unreliable_data_transfer(..)
            .collect::<Vec<_>>()
        {
            debug!("Server: preparing to send {:#?}.", scheduled);
            let packet = self.antarc.create_unreliable_data_transfer(scheduled);
            debug!("Server: ready to send {:#?}", packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = packet.as_raw::<Server>();
                info!(
                    "Server: sent {:#?} bytes to {:#?}",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc
                .sent_data_transfer(packet, ReliabilityType::Unreliable);
        }
    }

    fn poll_reliable_data_transfer(&mut self) {
        for scheduled in self
            .antarc
            .drain_reliable_data_transfer(..)
            .collect::<Vec<_>>()
        {
            debug!("Server: preparing to send {:#?}.", scheduled);
            let packet = self.antarc.create_reliable_data_transfer(scheduled);
            debug!("Server: ready to send {:#?}", packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = packet.as_raw::<Server>();
                info!(
                    "Server: sent {:#?} bytes to {:#?}",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc
                .sent_data_transfer(packet, ReliabilityType::Reliable);
        }
    }

    fn poll_unreliable_fragment(&mut self) {
        for scheduled in self
            .antarc
            .drain_unreliable_fragment(..)
            .collect::<Vec<_>>()
        {
            debug!("Server: preparing to send {:#?}.", scheduled);
            let packet = self.antarc.create_unreliable_fragment(scheduled);
            debug!("Server: ready to send {:#?}", packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = packet.as_raw::<Server>();
                info!(
                    "Server: sent {:#?} bytes to {:#?}",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc
                .sent_fragment(packet, ReliabilityType::Unreliable);
        }
    }

    fn poll_reliable_fragment(&mut self) {
        for scheduled in self.antarc.drain_reliable_fragment(..).collect::<Vec<_>>() {
            debug!("Server: preparing to send {:#?}.", scheduled);
            let packet = self.antarc.create_reliable_fragment(scheduled);
            debug!("Server: ready to send {:#?}", packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = packet.as_raw::<Server>();
                info!(
                    "Server: sent {:#?} bytes to {:#?}",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc.sent_fragment(packet, ReliabilityType::Reliable);
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

        // NOTE(alex): Dummy receive.
        for received in self.dummy_receiver.drain(..) {
            let raw_received = RawPacket::new("127.0.0.1:8888".parse().unwrap(), received);

            if let Err(fail) = self.antarc.on_received(raw_received) {
                error!("Server: encountered error on received {:#?}.", fail);
                self.antarc.service.api.push(ProtocolEvent::Fail(fail));
            }
        }

        self.antarc.poll()
    }

    fn poll_unreliable_heartbeat(&mut self) {
        for scheduled in self
            .antarc
            .drain_unreliable_heartbeat(..)
            .collect::<Vec<_>>()
        {
            debug!("Server: preparing to send {:#?}.", scheduled);
            let packet = self.antarc.create_unreliable_heartbeat(scheduled);
            debug!("Server: ready to send {:#?}.", packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = packet.as_raw::<Server>();
                info!(
                    "Server: sent {:#?} bytes to {:#?}.",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc
                .sent_heartbeat(packet, ReliabilityType::Unreliable);
        }
    }

    fn poll_reliable_heartbeat(&mut self) {
        for scheduled in self.antarc.drain_reliable_heartbeat(..).collect::<Vec<_>>() {
            debug!("Server: preparing to send {:#?}.", scheduled);
            let packet = self.antarc.create_reliable_heartbeat(scheduled);
            debug!("Server: ready to send {:#?}.", packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = packet.as_raw::<Server>();
                info!(
                    "Server: sent {:#?} bytes to {:#?}.",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc
                .sent_heartbeat(packet, ReliabilityType::Reliable);
        }
    }
}
