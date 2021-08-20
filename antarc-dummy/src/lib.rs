use std::net::SocketAddr;

use antarc_protocol::Service;
// TODO(alex) [mid] 2021-07-31: Create a dummy network manager implementation, that just uses
// buffers to move data, no sockets involved!
pub use antarc_protocol::{client::*, errors::*, events::*, packets::*, peers::*, server::*, *};
use log::*;

#[derive(Debug)]
pub struct DummyManager<S: Service> {
    antarc: Protocol<S>,
    address: SocketAddr,
    pub dummy_sender: Vec<Vec<u8>>,
    pub dummy_receiver: Vec<Vec<u8>>,
}

impl DummyManager<Server> {
    pub fn new_server(address: SocketAddr) -> Self {
        let antarc = Protocol::new_server();
        let dummy_sender = Vec::with_capacity(100);
        let dummy_receiver = Vec::with_capacity(100);
        Self {
            antarc,
            address,
            dummy_sender,
            dummy_receiver,
        }
    }

    pub fn schedule(
        &mut self,
        reliability: ReliabilityType,
        send_to: SendTo,
        payload: Payload,
    ) -> Result<PacketId, ProtocolError> {
        info!(
            "Server: scheduling transfer of {:#?} bytes, is reliable {:#?}, to {:#?}.",
            payload.len(),
            reliability,
            send_to,
        );
        self.antarc.schedule(reliability, send_to, payload)
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

        // TODO(alex) [high] 2021-08-20: Still missing `Heartbeat` packet handling.
        self.poll_retry_connection_accepted();
        self.poll_retry_data_transfer();
        self.poll_retry_fragment();
        self.poll_connection_accepted();
        self.poll_unreliable_data_transfer();
        self.poll_reliable_data_transfer();
        self.poll_unreliable_fragment();
        self.poll_reliable_fragment();

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
}

impl DummyManager<Client> {
    pub fn new_client(address: SocketAddr) -> Self {
        let antarc = Protocol::new_client();
        let dummy_sender = Vec::with_capacity(100);
        let dummy_receiver = Vec::with_capacity(100);
        Self {
            antarc,
            address,
            dummy_sender,
            dummy_receiver,
        }
    }

    // TODO(alex) [mid] 2021-08-02: Remember to return a `PacketId` from scheduling functions, so
    // that the user may cancel a packet.
    pub fn schedule(
        &mut self,
        reliability: ReliabilityType,
        payload: Payload,
    ) -> Result<PacketId, ProtocolError> {
        info!(
            "Client: scheduling transfer of {:#?} bytes, is reliable {:#?}.",
            payload.len(),
            reliability
        );

        self.antarc.schedule(reliability, payload)
    }

    pub fn connect(&mut self, remote_address: SocketAddr) -> Result<(), ProtocolError> {
        info!("Client: connect to {:#?}.", remote_address);
        self.antarc.connect(remote_address)
    }

    pub fn poll(&mut self) -> std::vec::Drain<ProtocolEvent<ClientEvent>> {
        info!("Client: dummy poll");

        // TODO(alex) [high] 2021-08-20: Still missing `Heartbeat` packet handling.
        self.poll_retry_connection_request();
        self.poll_retry_data_transfer();
        self.poll_connection_request();
        self.poll_unreliable_data_transfer();
        self.poll_reliable_data_transfer();
        self.poll_unreliable_fragment();
        self.poll_reliable_fragment();

        // NOTE(alex): Dummy receive.
        for received in self.dummy_receiver.drain(..) {
            let raw_received = RawPacket::new("127.0.0.1:7777".parse().unwrap(), received);
            if let Err(fail) = self.antarc.on_received(raw_received) {
                error!("Client: encountered error on received {:#?}.", fail);
                self.antarc.service.api.push(ProtocolEvent::Fail(fail));
            }
        }

        // TODO(alex) [vhigh] 2021-08-02: We have the handshake completed, but our dummy here
        // doesn't actually implement message passing, so the client and server do not communicate.
        //
        // I need a way of passing data between the two.

        self.antarc.poll()
    }

    fn poll_reliable_fragment(&mut self) {
        for scheduled in self.antarc.drain_reliable_fragment(..).collect::<Vec<_>>() {
            debug!("Client: preparing to send {:#?}.", scheduled);
            // TODO(alex) [low] 2021-08-17: The whole chain for this function is filled with
            // unneccesary duplication. Creation of unreliable / reliable packets are equal, the
            // only differences in reliability come AFTER the packet is sent.
            //
            // This means that these `create_` functions could take `<R: Reliability>` or something
            // generic like that, to avoid the need for 2 distinct function definitions.
            //
            // The duplication also applies to other `create_x` functions that are basically the
            // same for both Client and Server, but right now are completely separated.
            //
            // Most `drain_x` functions could be done at `impl<S: Service> Protocol<S>`.
            //
            // TODO(alex) [low] 2021-08-17: Could we get rid of duplication by passing down a
            // function callback?
            // fn common_create_data_transfer(scheduled, fn_create_reliable_data_transfer);
            let packet = self.antarc.create_reliable_fragment(scheduled);
            debug!("Client: ready to send {:#?}.", packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = packet.as_raw::<Client>();
                info!(
                    "Client: sent {:#?} bytes to {:#?}.",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc.sent_fragment(packet, ReliabilityType::Reliable);
        }
    }

    fn poll_reliable_data_transfer(&mut self) {
        for scheduled in self
            .antarc
            .drain_reliable_data_transfer(..)
            .collect::<Vec<_>>()
        {
            debug!("Client: preparing to send {:#?}.", scheduled);
            // TODO(alex) [low] 2021-08-17: The whole chain for this function is filled with
            // unneccesary duplication. Creation of unreliable / reliable packets are equal, the
            // only differences in reliability come AFTER the packet is sent.
            //
            // This means that these `create_` functions could take `<R: Reliability>` or something
            // generic like that, to avoid the need for 2 distinct function definitions.
            //
            // The duplication also applies to other `create_x` functions that are basically the
            // same for both Client and Server, but right now are completely separated.
            //
            // Most `drain_x` functions could be done at `impl<S: Service> Protocol<S>`.
            //
            // TODO(alex) [low] 2021-08-17: Could we get rid of duplication by passing down a
            // function callback?
            // fn common_create_data_transfer(scheduled, fn_create_reliable_data_transfer);
            let packet = self.antarc.create_reliable_data_transfer(scheduled);
            debug!("Client: ready to send {:#?}.", packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = packet.as_raw::<Client>();
                info!(
                    "Client: sent {:#?} bytes to {:#?}.",
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
            debug!("Client: preparing to send {:#?}.", scheduled);
            let packet = self.antarc.create_unreliable_fragment(scheduled);
            debug!("Client: ready to send {:#?}.", packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = packet.as_raw::<Client>();
                info!(
                    "Client: sent {:#?} bytes to {:#?}.",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc
                .sent_fragment(packet, ReliabilityType::Unreliable);
        }
    }

    fn poll_unreliable_data_transfer(&mut self) {
        for scheduled in self
            .antarc
            .drain_unreliable_data_transfer(..)
            .collect::<Vec<_>>()
        {
            debug!("Client: preparing to send {:#?}.", scheduled);
            let packet = self.antarc.create_unreliable_data_transfer(scheduled);
            debug!("Client: ready to send {:#?}.", packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = packet.as_raw::<Client>();
                info!(
                    "Client: sent {:#?} bytes to {:#?}.",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc
                .sent_data_transfer(packet, ReliabilityType::Unreliable);
        }
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

            // NOTE(alex): Dummy send.
            {
                let raw_packet = packet.as_raw::<Client>();
                info!(
                    "Client: sent {:#?} bytes to {:#?}",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc.sent_connection_request(packet);
        }
    }

    fn poll_retry_data_transfer(&mut self) {
        if let Some(reliable_packet) = self.antarc.retry_reliable_data_transfer() {
            debug!("Client: ready to re-send {:#?}", reliable_packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = reliable_packet.as_raw::<Server>();
                info!(
                    "Client: re-sent {:#?} bytes to {:#?}",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc
                .sent_data_transfer(reliable_packet, ReliabilityType::Reliable);
        }
    }

    fn poll_retry_connection_request(&mut self) {
        if let Some(reliable_packet) = self.antarc.resend_reliable_connection_request() {
            debug!("Client: ready to re-send {:#?}", reliable_packet);

            // NOTE(alex): Dummy send.
            {
                let raw_packet = reliable_packet.as_raw::<Server>();
                info!(
                    "Client: re-sent {:#?} bytes to {:#?}",
                    raw_packet.bytes.len(),
                    raw_packet.address
                );
                self.dummy_sender.push(raw_packet.bytes);
            }

            self.antarc.sent_connection_request(reliable_packet);
        }
    }
}
