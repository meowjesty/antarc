use std::{sync::Arc, time::Instant};

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

    // TODO(alex) [mid] 2021-08-02: There must be a way to have a generic version of this function.
    // If `Messager` or some other `Packet` trait implements an `Into<SentEvent>` it would work.
    //
    // Check the `client.rs` file that contains a comment with this possible function.
    pub fn sent_connection_accepted(&mut self, packet: Packet<ToSend, ConnectionAccepted>) {
        self.service
            .sent_connection_accepted(packet, self.timer.elapsed());
    }

    pub fn create_connection_accepted(
        &self,
        scheduled: Scheduled<Reliable, ConnectionAccepted>,
    ) -> Packet<ToSend, ConnectionAccepted> {
        self.service
            .create_connection_accepted(scheduled, self.timer.elapsed())
    }

    // REGION(alex): Data Transfer
    pub fn create_unreliable_data_transfer(
        &self,
        scheduled: Scheduled<Unreliable, DataTransfer>,
    ) -> Packet<ToSend, DataTransfer> {
        self.service
            .create_unreliable_data_transfer(scheduled, self.timer.elapsed())
    }

    pub fn sent_data_transfer(&mut self, packet: Packet<ToSend, DataTransfer>) {
        self.service
            .sent_data_transfer(packet, self.timer.elapsed())
    }

    /// NOTE(alex): API function for scheduling data transfers only, called by the user.
    ///
    /// There are 2 choices:
    ///
    /// 1. schedule to single peer;
    /// 2. broadcast to every peer;
    ///
    /// If the user wants to send to multiple select peers, then they must call the single version
    /// multiple times.
    ///
    /// TODO(alex) [low] 2021-08-08: Most of the duplication problems were solved by moving into a
    /// separate helper function, a bit remains though.
    ///
    /// TODO(alex) [vlow] 2021-08-09: There was a `SendTo::Multiple` that took a list of connection
    /// ids, but this function can't properly handle failure state for this variant. To schedule for
    /// multiple peers like that, another return type must be had in case the user passes 1 or more
    /// not-connected connection ids. This could be some sort of `schedule_batch`.
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

    pub fn poll(&mut self) -> std::vec::Drain<ProtocolEvent<ServerEvent>> {
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
