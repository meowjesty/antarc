use core::time::Duration;
use std::{net::SocketAddr, time::Instant};

use crate::{errors::*, events::*, packets::*, Protocol};

pub(crate) mod service;

pub use service::*;

impl Protocol<Client> {
    pub fn new_client() -> Self {
        let service = Client::new();

        Self {
            packet_id_tracker: 0,
            timer: Instant::now(),
            service,
            receiver_pipe: Vec::with_capacity(32),
            reliable_ttl: Duration::from_secs(2),
        }
    }

    // REGION(alex): Connection Request
    pub fn create_connection_request(
        &self,
        scheduled: Scheduled<Reliable, ConnectionRequest>,
    ) -> Packet<ToSend, ConnectionRequest> {
        self.service
            .create_connection_request(scheduled, self.timer.elapsed())
    }

    /// TODO(alex) [mid] 2021-08-02: There must be a way to have a generic version of this function.
    /// If `Messager` or some other `Packet` trait implements an `Into<SentEvent>` it would work.
    pub fn sent_connection_request(&mut self, packet: Packet<ToSend, ConnectionRequest>) {
        self.service
            .sent_connection_request(packet, self.timer.elapsed(), self.reliable_ttl);
    }

    pub fn sent_data_transfer(
        &mut self,
        packet: Packet<ToSend, DataTransfer>,
        reliability: ReliabilityType,
    ) {
        self.service.sent_data_transfer(
            packet,
            self.timer.elapsed(),
            reliability,
            self.reliable_ttl,
        )
    }

    pub fn sent_heartbeat(
        &mut self,
        packet: Packet<ToSend, Heartbeat>,
        reliability: ReliabilityType,
    ) {
        self.service
            .sent_heartbeat(packet, self.timer.elapsed(), reliability, self.reliable_ttl)
    }

    pub fn sent_fragment(
        &mut self,
        packet: Packet<ToSend, Fragment>,
        reliability: ReliabilityType,
    ) {
        self.service
            .sent_fragment(packet, self.timer.elapsed(), reliability, self.reliable_ttl)
    }

    /// NOTE(alex): API function that feeds the internal* event pipe.
    pub fn on_received(&mut self, raw_packet: RawPacket<Client>) -> Result<(), ProtocolError> {
        self.service.on_received(raw_packet, self.timer.elapsed())
    }

    pub fn connect(&mut self, remote_address: SocketAddr) -> Result<(), ProtocolError> {
        let result =
            self.service
                .connect(remote_address, self.packet_id_tracker, self.timer.elapsed())?;
        self.packet_id_tracker += 1;

        Ok(result)
    }

    pub fn heartbeat(&mut self, reliability: ReliabilityType) -> Result<PacketId, ProtocolError> {
        let packet_id =
            self.service
                .heartbeat(reliability, self.packet_id_tracker, self.timer.elapsed())?;

        self.packet_id_tracker += 1;

        Ok(packet_id)
    }

    pub fn resend_reliable_connection_request(
        &mut self,
    ) -> Option<Packet<ToSend, ConnectionRequest>> {
        self.service
            .resend_reliable_connection_request(self.timer.elapsed())
    }

    pub fn poll(&mut self) -> std::vec::Drain<ProtocolEvent<ClientEvent>> {
        self.service.reliability_handler.poll(self.timer.elapsed());

        self.service.api.drain(..)
    }
}
