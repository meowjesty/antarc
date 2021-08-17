use std::{net::SocketAddr, sync::Arc, time::Instant};

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
            .sent_connection_request(packet, self.timer.elapsed());
    }

    // REGION(alex): Data Transfer
    pub fn create_unreliable_data_transfer(
        &self,
        scheduled: Scheduled<Unreliable, DataTransfer>,
    ) -> Packet<ToSend, DataTransfer> {
        self.service
            .create_unreliable_data_transfer(scheduled, self.timer.elapsed())
    }

    pub fn create_reliable_data_transfer(
        &self,
        scheduled: Scheduled<Reliable, DataTransfer>,
    ) -> Packet<ToSend, DataTransfer> {
        self.service
            .create_reliable_data_transfer(scheduled, self.timer.elapsed())
    }

    pub fn sent_data_transfer(
        &mut self,
        packet: Packet<ToSend, DataTransfer>,
        reliability: ReliabilityType,
    ) {
        self.service
            .sent_data_transfer(packet, self.timer.elapsed(), reliability)
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

    /// NOTE(alex): API function for scheduling data transfers only, called by the user.
    pub fn schedule(
        &mut self,
        reliability: ReliabilityType,
        payload: Payload,
    ) -> Result<PacketId, ProtocolError> {
        let payload = Arc::new(payload);
        let packet_id = self.service.schedule(
            reliability,
            payload,
            self.packet_id_tracker,
            self.timer.elapsed(),
        )?;

        self.packet_id_tracker += 1;

        Ok(packet_id)
    }

    pub fn resend_reliable_connection_request(
        &mut self,
    ) -> Option<Packet<ToSend, ConnectionRequest>> {
        self.service
            .resend_reliable_connection_request(self.timer.elapsed())
    }

    pub fn resend_reliable_data_transfer(&mut self) -> Option<Packet<ToSend, DataTransfer>> {
        self.service
            .resend_reliable_data_transfer(self.timer.elapsed())
    }

    pub fn resend_reliable_fragment(&mut self) -> Option<Packet<ToSend, Fragment>> {
        self.service.resend_reliable_fragment(self.timer.elapsed())
    }

    pub fn poll(&mut self) -> std::vec::Drain<ProtocolEvent<ClientEvent>> {
        // TODO(alex) [mid] 2021-07-30: Handle `scheduler` pipe of events. But how exactly?
        // The network will check if socket is ready, then call a `make_packet` that will take some
        // scheduled from the scheduler pipe, but how does it handle reliability?
        //
        // ADD(alex) [mid] 2021-08-01: After sending a reliable packet, it should be put in a list
        // of `SentReliable` or whatever packets, and these packets are checked via their
        // `meta.time` and resent after some time has passed, and they've not been acked yet.
        //
        // No need to convert such a packet into a `Scheduled`, the easier approach would be to have
        // a secondary `send_non_acked` function, that runs before the common `send`, and it takes
        // a packet from this reliable list and re-sends it, with updated time.

        self.service.api.drain(..)
    }
}
