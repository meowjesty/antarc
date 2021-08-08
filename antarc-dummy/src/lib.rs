use std::net::SocketAddr;

// TODO(alex) [mid] 2021-07-31: Create a dummy network manager implementation, that just uses
// buffers to move data, no sockets involved!
pub use antarc_protocol::{
    client::Client,
    events::AntarcEvent,
    packets::{ConnectionId, Payload, ReliabilityType},
    peers::SendTo,
    server::Server,
    Protocol,
};
use antarc_protocol::{events::*, packets::*};
use log::{debug, error, info, warn};

#[derive(Debug)]
pub struct DummyManager<Service> {
    antarc: Protocol<Service>,
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

    pub fn poll(&mut self) -> std::vec::Drain<AntarcEvent> {
        debug!("Server: dummy poll");

        for scheduled in self.antarc.scheduler().drain(..) {
            match scheduled {
                ScheduleEvent::ConnectionRequest { scheduled } => {
                    warn!("Server: invalid packet type {:#?} scheduled.", scheduled);
                    self.antarc
                        .events
                        .api
                        .push(ProtocolError::ScheduledInvalidPeer(scheduled.packet_id).into());
                }
                ScheduleEvent::ConnectionAccepted { scheduled } => {
                    debug!("Server: preparing to send {:#?}.", scheduled);
                    let packet = self.antarc.prepare_connection_accepted(scheduled);
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
                ScheduleEvent::ReliableDataTransfer { .. } => todo!(),
                ScheduleEvent::ReliableFragment { .. } => todo!(),
                ScheduleEvent::UnreliableDataTransfer { .. } => {
                    // TODO(alex) [high] 2021-08-05: We have a handshake of sorts, now it's time
                    // to implement the other messages.
                    todo!()
                }
                ScheduleEvent::UnreliableFragment { .. } => todo!(),
            }
        }

        // NOTE(alex): Dummy receive.
        for received in self.dummy_receiver.drain(..) {
            let raw_received = RawPacket::new("127.0.0.1:8888".parse().unwrap(), received);
            if let Err(fail) = self.antarc.on_received(raw_received) {
                error!("Server: encountered error on received {:#?}.", fail);
                self.antarc.events.api.push(AntarcEvent::Fail(fail));
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

    pub fn poll(&mut self) -> std::vec::Drain<AntarcEvent> {
        info!("Client: dummy poll");

        for scheduled in self.antarc.scheduler().drain(..) {
            match scheduled {
                ScheduleEvent::ConnectionRequest { scheduled } => {
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
                ScheduleEvent::ConnectionAccepted { scheduled } => {
                    warn!("Client: invalid packet type {:#?} scheduled.", scheduled);
                    self.antarc
                        .events
                        .api
                        .push(ProtocolError::ScheduledInvalidPeer(scheduled.packet_id).into());
                }
                ScheduleEvent::ReliableDataTransfer { .. } => todo!(),
                ScheduleEvent::ReliableFragment { .. } => todo!(),
                ScheduleEvent::UnreliableDataTransfer { scheduled } => {
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

                    self.antarc.sent_data_transfer(packet);
                }
                ScheduleEvent::UnreliableFragment { .. } => todo!(),
            }
        }

        // NOTE(alex): Dummy receive.
        for received in self.dummy_receiver.drain(..) {
            let raw_received = RawPacket::new("127.0.0.1:7777".parse().unwrap(), received);
            if let Err(fail) = self.antarc.on_received(raw_received) {
                error!("Client: encountered error on received {:#?}.", fail);
                self.antarc.events.api.push(AntarcEvent::Fail(fail));
            }
        }

        // TODO(alex) [vhigh] 2021-08-02: We have the handshake completed, but our dummy here
        // doesn't actually implement message passing, so the client and server do not communicate.
        //
        // I need a way of passing data between the two.

        self.antarc.poll()
    }
}
