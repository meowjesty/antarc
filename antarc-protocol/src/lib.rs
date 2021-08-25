#![feature(duration_consts_2)]
#![feature(drain_filter)]
#![feature(hash_drain_filter)]
#![feature(nonzero_ops)]
// #![feature(const_generics)]
// #![feature(evaluate_checked)]
// #![feature(const_try)]
// #![feature(array_chunks)]
#![allow(clippy::let_and_return)]

use core::mem::size_of;
use std::{
    num::NonZeroU32,
    ops::RangeBounds,
    sync::Arc,
    time::{Duration, Instant},
    vec::Drain,
};

use errors::ProtocolError;
use log::debug;
use packets::*;
use peers::*;

use self::{reliability::*, service_traits::*};
use crate::packets::{delivery::*, message::*, raw::*, scheduled::*};

pub mod client;
pub mod errors;
pub mod events;
pub mod packets;
pub mod peers;
pub mod reliability;
pub mod scheduler;
pub mod server;
pub mod service_traits;

#[macro_export]
macro_rules! read_buffer_inc {
    ({ $buffer: expr, $start: expr } : $kind: ident) => {{
        let end = $start + size_of::<$kind>();
        let bytes_arr: &[u8; size_of::<$kind>()] = $buffer[$start..end].try_into()?;
        let val = $kind::from_be_bytes(*bytes_arr);
        $start = end;
        val
    }};
}

#[macro_export]
macro_rules! scheduler {
    ({$packet_id: expr, $connection_id: expr, $payload: expr, $fragment_index: expr,
        $fragment_total: expr, $time: expr, $address: expr} :$reliability: ident) => {{
        let scheduled: Scheduled<$reliability, Fragment> = Scheduled::new(
            $packet_id,
            $connection_id,
            $reliability {},
            $payload,
            $fragment_index,
            $fragment_total,
            $time,
            $address,
        );

        scheduled
    }};
}

pub type ProtocolId = NonZeroU32;

pub const PROTOCOL_ID: ProtocolId = unsafe { ProtocolId::new_unchecked(0xbabedad) };
pub const PROTOCOL_ID_BYTES: [u8; size_of::<ProtocolId>()] = PROTOCOL_ID.get().to_be_bytes();

/// NOTE(alex): `Service` may be either a `Client` or a `Server`.
#[derive(Debug)]
pub struct Protocol<S>
where
    S: Service,
{
    pub packet_id_tracker: PacketId,
    pub timer: Instant,
    pub service: S,
    pub receiver_pipe: Vec<RawPacket<S>>,
    pub reliable_ttl: Duration,
}

impl<S> Protocol<S>
where
    S: Service,
{
    pub fn cancel_packet(&mut self, _packet_id: PacketId, _reliability: ReliabilityType) -> bool {
        todo!()
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

    pub fn sent_heartbeat(
        &mut self,
        packet: Packet<ToSend, Heartbeat>,
        reliability: ReliabilityType,
    ) {
        self.service
            .sent_heartbeat(packet, self.timer.elapsed(), reliability, self.reliable_ttl)
    }

    pub fn create_unreliable_data_transfer(
        &self,
        scheduled: Scheduled<Unreliable, DataTransfer>,
    ) -> Packet<ToSend, DataTransfer> {
        let (sequence, ack) = self
            .service
            .connected()
            .get(&scheduled.message.connection_id)
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .expect("Creating a packet (unreliable data transfer) should never fail!");

        let packet = scheduled.into_packet(sequence, ack, self.timer.elapsed());
        packet
    }

    pub fn create_reliable_data_transfer(
        &self,
        scheduled: Scheduled<Reliable, DataTransfer>,
    ) -> Packet<ToSend, DataTransfer> {
        let (sequence, ack) = self
            .service
            .connected()
            .get(&scheduled.message.connection_id)
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .expect("Creating a packet (unreliable data transfer) should never fail!");

        let packet = scheduled.into_packet(sequence, ack, self.timer.elapsed());
        packet
    }

    pub fn drain_unreliable_data_transfer<R: RangeBounds<usize>>(
        &mut self,
        range: R,
    ) -> Drain<Scheduled<Unreliable, DataTransfer>> {
        self.service
            .scheduler_mut()
            .list_scheduled_unreliable_data_transfer
            .drain(range)
    }

    pub fn drain_reliable_data_transfer<R: RangeBounds<usize>>(
        &mut self,
        range: R,
    ) -> Drain<Scheduled<Reliable, DataTransfer>> {
        self.service
            .scheduler_mut()
            .list_scheduled_reliable_data_transfer
            .drain(range)
    }

    pub fn retry_reliable_data_transfer(&mut self) -> Option<Packet<ToSend, DataTransfer>> {
        self.service
            .reliability_handler_mut()
            .retry_reliable_data_transfer(self.timer.elapsed())
    }

    pub fn create_unreliable_fragment(
        &self,
        scheduled: Scheduled<Unreliable, Fragment>,
    ) -> Packet<ToSend, Fragment> {
        let (sequence, ack) = self
            .service
            .connected()
            .get(&scheduled.message.connection_id)
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .expect("Creating a packet (unreliable data transfer) should never fail!");

        let packet = scheduled.into_packet(sequence, ack, self.timer.elapsed());
        packet
    }

    pub fn create_reliable_fragment(
        &self,
        scheduled: Scheduled<Reliable, Fragment>,
    ) -> Packet<ToSend, Fragment> {
        let (sequence, ack) = self
            .service
            .connected()
            .get(&scheduled.message.connection_id)
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .expect("Creating a packet (unreliable data transfer) should never fail!");

        let packet = scheduled.into_packet(sequence, ack, self.timer.elapsed());
        packet
    }

    pub fn create_unreliable_heartbeat(
        &self,
        scheduled: Scheduled<Unreliable, Heartbeat>,
    ) -> Packet<ToSend, Heartbeat> {
        let (sequence, ack) = self
            .service
            .connected()
            .get(&scheduled.message.connection_id)
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .expect("Creating a packet (heartbeat) should never fail!");

        let packet = scheduled.into_packet(sequence, ack, self.timer.elapsed());
        packet
    }

    pub fn create_reliable_heartbeat(
        &self,
        scheduled: Scheduled<Reliable, Heartbeat>,
    ) -> Packet<ToSend, Heartbeat> {
        let (sequence, ack) = self
            .service
            .connected()
            .get(&scheduled.message.connection_id)
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .expect("Creating a packet (heartbeat) should never fail!");

        let packet = scheduled.into_packet(sequence, ack, self.timer.elapsed());
        packet
    }

    pub fn drain_unreliable_fragment<R: RangeBounds<usize>>(
        &mut self,
        range: R,
    ) -> Drain<Scheduled<Unreliable, Fragment>> {
        self.service
            .scheduler_mut()
            .list_scheduled_unreliable_fragment
            .drain(range)
    }

    pub fn drain_reliable_fragment<R: RangeBounds<usize>>(
        &mut self,
        range: R,
    ) -> Drain<Scheduled<Reliable, Fragment>> {
        self.service
            .scheduler_mut()
            .list_scheduled_reliable_fragment
            .drain(range)
    }

    pub fn drain_reliable_heartbeat<R: RangeBounds<usize>>(
        &mut self,
        range: R,
    ) -> Drain<Scheduled<Reliable, Heartbeat>> {
        self.service
            .scheduler_mut()
            .list_scheduled_reliable_heartbeat
            .drain(range)
    }

    pub fn drain_unreliable_heartbeat<R: RangeBounds<usize>>(
        &mut self,
        range: R,
    ) -> Drain<Scheduled<Unreliable, Heartbeat>> {
        self.service
            .scheduler_mut()
            .list_scheduled_unreliable_heartbeat
            .drain(range)
    }

    pub fn retry_reliable_fragment(&mut self) -> Option<Packet<ToSend, Fragment>> {
        self.service
            .reliability_handler_mut()
            .retry_reliable_fragment(self.timer.elapsed())
    }

    pub fn retry_reliable_heartbeat(&mut self) -> Option<Packet<ToSend, Heartbeat>> {
        self.service
            .reliability_handler_mut()
            .retry_reliable_heartbeat(self.timer.elapsed())
    }

    /// NOTE(alex): API function for scheduling data transfers only, called by the user.
    pub fn schedule(
        &mut self,
        reliability: ReliabilityType,
        send_to: SendTo,
        payload: Payload,
    ) -> Result<PacketId, ProtocolError> {
        let packet_id = self.schedule_helper(
            reliability,
            send_to,
            payload,
            self.packet_id_tracker,
            self.timer.elapsed(),
        )?;

        self.packet_id_tracker += 1;

        Ok(packet_id)
    }

    pub fn heartbeat(
        &mut self,
        reliability: ReliabilityType,
        send_to: SendTo,
    ) -> Result<PacketId, ProtocolError> {
        let packet_id = self.heartbeat_helper(
            reliability,
            send_to,
            self.packet_id_tracker,
            self.timer.elapsed(),
        )?;

        self.packet_id_tracker += 1;

        Ok(packet_id)
    }

    /// NOTE(alex): Helper function for scheduling data transfers only.
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
    fn schedule_helper(
        &mut self,
        reliability: ReliabilityType,
        send_to: SendTo,
        payload: Payload,
        packet_id: PacketId,
        time: Duration,
    ) -> Result<PacketId, ProtocolError> {
        if self.service.connected().is_empty() {
            return Err(ProtocolError::NoPeersConnected);
        }

        let fragments = payload
            .chunks(MAX_FRAGMENT_SIZE - Fragment::HEADER_SIZE)
            .enumerate()
            // TODO(alex) [mid] 2021-08-17: Change `Payload` to be `Arc<&[u8]>` so we don't need to
            // use `to_vec` here.
            //
            // ADD(alex) [low] 2021-08-18: This change has a big impact, and I don't think it's
            // possible to remove the allocation anyway, as `Arc::from(chunk)` would involve a
            // memcpy of `chunk`.
            // https://github.com/rust-lang/rust/pull/42565
            .map(|(index, chunk)| (index, Arc::new(chunk.to_vec())))
            .collect::<Vec<_>>();

        let fragment_total = fragments.len();

        for (fragment_index, payload) in fragments.into_iter() {
            match send_to {
                SendTo::Single { connection_id } => {
                    debug!("SendTo::Single scheduling for {:#?}.", connection_id);

                    let address = self
                        .service
                        .connected()
                        .get(&connection_id)
                        .map(|peer| peer.address)
                        .ok_or(ProtocolError::ScheduledNotConnected(connection_id))?;

                    self.service.scheduler_mut().schedule_for_connected_peer(
                        address,
                        payload.clone(),
                        reliability,
                        connection_id,
                        packet_id,
                        time,
                        fragment_index,
                        fragment_total,
                    );
                }
                SendTo::Broadcast => {
                    // TODO(alex) [mid] 2021-08-21: If I don't collect here, we end up with a double
                    // borrow of `self.service`, even though we're borrowing fields that do not
                    // interact.
                    let connected = self
                        .service
                        .connected()
                        .iter()
                        .map(|(connection_id, peer)| (*connection_id, peer.address))
                        .collect::<Vec<_>>();
                    let scheduler = self.service.scheduler_mut();

                    for (connection_id, address) in connected {
                        debug!("SendTo::Broadcast scheduling for {:#?}.", connection_id);
                        scheduler.schedule_for_connected_peer(
                            address,
                            payload.clone(),
                            reliability,
                            connection_id,
                            packet_id,
                            time,
                            fragment_index,
                            fragment_total,
                        );
                    }
                }
            }
        }

        Ok(packet_id + 1)
    }

    fn heartbeat_helper(
        &mut self,
        reliability: ReliabilityType,
        send_to: SendTo,
        packet_id: PacketId,
        time: Duration,
    ) -> Result<PacketId, ProtocolError> {
        if self.service.connected().is_empty() {
            return Err(ProtocolError::NoPeersConnected);
        }

        match send_to {
            SendTo::Single { connection_id } => {
                debug!(
                    "server: SendTo::Single scheduling for {:#?}.",
                    connection_id
                );

                let address = self
                    .service
                    .connected()
                    .get(&connection_id)
                    .map(|peer| peer.address)
                    .ok_or(ProtocolError::ScheduledNotConnected(connection_id))?;

                self.service.scheduler_mut().heartbeat_for_connected_peer(
                    address,
                    reliability,
                    connection_id,
                    packet_id,
                    time,
                );
            }
            SendTo::Broadcast => {
                // TODO(alex) [mid] 2021-08-21: If I don't collect here, we end up with a double
                // borrow of `self.service`, even though we're borrowing fields that do not
                // interact.
                let connected = self
                    .service
                    .connected()
                    .iter()
                    .map(|(connection_id, peer)| (*connection_id, peer.address))
                    .collect::<Vec<_>>();
                let scheduler = self.service.scheduler_mut();

                for (connection_id, address) in connected {
                    debug!(
                        "server: SendTo::Broadcast scheduling for {:#?}.",
                        connection_id
                    );
                    scheduler.heartbeat_for_connected_peer(
                        address,
                        reliability,
                        connection_id,
                        packet_id,
                        time,
                    );
                }
            }
        }

        Ok(packet_id + 1)
    }
}
