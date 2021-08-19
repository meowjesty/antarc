use core::time::Duration;
use std::time::Instant;

use crate::{errors::*, events::*, packets::*, peers::*, Protocol};

pub mod service;

pub use service::*;

impl Protocol<Server> {
    pub fn new_server() -> Self {
        let service = Server::new();

        Self {
            packet_id_tracker: 0,
            timer: Instant::now(),
            service,
            receiver_pipe: Vec::with_capacity(32),
            reliable_ttl: Duration::from_secs(2),
        }
    }

    /// NOTE(alex): API function that feeds the internal* event pipe.
    pub fn on_received(&mut self, raw_packet: RawPacket<Server>) -> Result<(), ProtocolError> {
        let update_packet_id =
            self.service
                .on_received(raw_packet, self.packet_id_tracker, self.timer.elapsed())?;

        if update_packet_id {
            self.packet_id_tracker += 1;
        }

        Ok(())
    }

    // REGION(alex): Connection Accepted
    pub fn create_connection_accepted(
        &self,
        scheduled: Scheduled<Reliable, ConnectionAccepted>,
    ) -> Packet<ToSend, ConnectionAccepted> {
        self.service
            .create_connection_accepted(scheduled, self.timer.elapsed())
    }

    // TODO(alex) [mid] 2021-08-02: There must be a way to have a generic version of this function.
    // If `Messager` or some other `Packet` trait implements an `Into<SentEvent>` it would work.
    //
    // Check the `client.rs` file that contains a comment with this possible function.
    pub fn sent_connection_accepted(&mut self, packet: Packet<ToSend, ConnectionAccepted>) {
        self.service
            .sent_connection_accepted(packet, self.timer.elapsed(), self.reliable_ttl);
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

    pub fn sent_fragment(
        &mut self,
        packet: Packet<ToSend, Fragment>,
        reliability: ReliabilityType,
    ) {
        self.service
            .sent_fragment(packet, self.timer.elapsed(), reliability, self.reliable_ttl)
    }

    /// NOTE(alex): API function for scheduling data transfers only, called by the user.
    ///
    /// There are 2 choices:
    ///
    /// 1. schedule to single peer;
    /// 2. broadcast to every peer;
    ///
    /// If the user wants to send to multiple select peers, then they must call the single version
    /// multiple times. This avoids introducing a special case for when this "send batch" contains
    /// an invalid `ConnectionId`.
    ///
    /// TODO(alex) [low] 2021-08-08: Most of the duplication problems were solved by moving into a
    /// separate helper function, a bit remains though.
    pub fn schedule(
        &mut self,
        reliability: ReliabilityType,
        send_to: SendTo,
        payload: Payload,
    ) -> Result<PacketId, ProtocolError> {
        let packet_id = self.service.schedule(
            reliability,
            send_to,
            payload,
            self.packet_id_tracker,
            self.timer.elapsed(),
        )?;

        self.packet_id_tracker += 1;

        Ok(packet_id)
    }

    pub fn retry_reliable_connection_accepted(
        &mut self,
    ) -> Option<Packet<ToSend, ConnectionAccepted>> {
        self.service
            .retry_reliable_connection_accepted(self.timer.elapsed())
    }

    pub fn poll(&mut self) -> std::vec::Drain<ProtocolEvent<ServerEvent>> {
        self.service.reliability_handler.poll(self.timer.elapsed());

        self.service.api.drain(..)
    }
}
