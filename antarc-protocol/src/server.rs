use core::time::Duration;
use std::time::Instant;

use crate::{
    errors::*,
    events::*,
    packets::{delivery::*, message::*, raw::*, scheduled::*, *},
    Protocol,
};

pub mod service;

pub use service::*;

impl Protocol<Server> {
    pub fn new_server(capacity: usize, reliable_ttl: Duration) -> Self {
        let service = Server::new(capacity);

        Self {
            packet_id_tracker: 0,
            timer: Instant::now(),
            service,
            receiver_pipe: Vec::with_capacity(capacity),
            reliable_ttl,
        }
    }

    /// NOTE(alex): API function that feeds the internal* event pipe.
    pub fn on_received(&mut self, raw_packet: RawPacket) -> Result<(), ProtocolError> {
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

    pub fn retry_reliable_connection_accepted(
        &mut self,
    ) -> Option<Packet<ToSend, ConnectionAccepted>> {
        self.service
            .retry_reliable_connection_accepted(self.timer.elapsed())
    }

    pub fn poll(&mut self) -> std::vec::Drain<ProtocolEvent<ServerEvent>> {
        self.service.reliability_handler.poll(self.timer.elapsed());

        // NOTE(alex): Iterate over connected peers and call `poll` on each to remove dead packets
        // in reassembler.
        let now = self.timer.elapsed();
        for peer in self.service.connected.values_mut() {
            peer.poll(now);
        }

        self.service.api.drain(..)
    }
}
