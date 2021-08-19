#![feature(duration_consts_2)]
#![feature(drain_filter)]
#![feature(hash_drain_filter)]
#![feature(nonzero_ops)]
// #![feature(const_generics)]
// #![feature(const_try)]
// #![feature(array_chunks)]

use core::mem::size_of;
use std::{
    collections::HashMap,
    net::SocketAddr,
    num::NonZeroU32,
    ops::RangeBounds,
    sync::Arc,
    time::{Duration, Instant},
    vec::Drain,
};

use log::debug;
use packets::*;
use peers::{Connected, Peer};

pub mod client;
pub mod errors;
pub mod events;
pub mod packets;
pub mod peers;
pub mod server;

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

pub trait Service {
    type SchedulerType: ServiceScheduler;
    type ReliabilityHandlerType: ServiceReliability;

    fn scheduler(&self) -> &Scheduler<Self::SchedulerType>;
    fn scheduler_mut(&mut self) -> &mut Scheduler<Self::SchedulerType>;

    fn reliability_handler(&self) -> &ReliabilityHandler<Self::ReliabilityHandlerType>;
    fn reliability_handler_mut(&mut self) -> &mut ReliabilityHandler<Self::ReliabilityHandlerType>;

    fn connected(&self) -> &HashMap<ConnectionId, Peer<Connected>>;
}

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

    pub fn retry_reliable_fragment(&mut self) -> Option<Packet<ToSend, Fragment>> {
        self.service
            .reliability_handler_mut()
            .retry_reliable_fragment(self.timer.elapsed())
    }
}

pub trait ServiceScheduler {
    fn new(capacity: usize) -> Self;
}

#[derive(Debug)]
pub struct Scheduler<S: ServiceScheduler> {
    list_scheduled_reliable_data_transfer: Vec<Scheduled<Reliable, DataTransfer>>,
    list_scheduled_unreliable_data_transfer: Vec<Scheduled<Unreliable, DataTransfer>>,
    list_scheduled_reliable_fragment: Vec<Scheduled<Reliable, Fragment>>,
    list_scheduled_unreliable_fragment: Vec<Scheduled<Unreliable, Fragment>>,

    service: S,
}

pub trait ServiceReliability {
    fn new(capacity: usize) -> Self;
    fn poll(&mut self, now: Duration);
}

#[derive(Debug)]
pub struct ReliabilityHandler<S: ServiceReliability> {
    list_sent_reliable_data_transfer: Vec<Packet<Sent, DataTransfer>>,
    list_sent_reliable_fragment: Vec<Packet<Sent, Fragment>>,

    service: S,
}

impl<S: ServiceReliability> ReliabilityHandler<S> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            list_sent_reliable_data_transfer: Vec::with_capacity(capacity),
            list_sent_reliable_fragment: Vec::with_capacity(capacity),
            service: S::new(capacity),
        }
    }

    fn poll(&mut self, now: Duration) {
        if let Some(_) = self
            .list_sent_reliable_data_transfer
            .first()
            .map(|packet| (packet.delivery.meta.time + packet.delivery.ttl > now).then(|| ()))
        {
            self.list_sent_reliable_data_transfer.remove(0);
        } else if let Some(_) = self
            .list_sent_reliable_fragment
            .first()
            .map(|packet| (packet.delivery.meta.time + packet.delivery.ttl > now).then(|| ()))
        {
            self.list_sent_reliable_fragment.remove(0);
        }

        self.service.poll(now);
    }

    pub(crate) fn retry_reliable_data_transfer(
        &mut self,
        now: Duration,
    ) -> Option<Packet<ToSend, DataTransfer>> {
        if let Some(packet) = self.list_sent_reliable_data_transfer.pop() {
            if packet.delivery.meta.time + now > Duration::from_secs(1000) {
                let meta = MetaDelivery {
                    time: now,
                    address: packet.delivery.meta.address,
                };
                let delivery = ToSend {
                    id: packet.delivery.id,
                    meta,
                };
                let message = DataTransfer {
                    meta: packet.message.meta,
                    connection_id: packet.message.connection_id,
                    payload: packet.message.payload,
                };
                let result = Packet {
                    delivery,
                    sequence: packet.sequence,
                    ack: packet.ack,
                    message,
                };

                return Some(result);
            }
        }

        None
    }

    pub(crate) fn retry_reliable_fragment(
        &mut self,
        now: Duration,
    ) -> Option<Packet<ToSend, Fragment>> {
        if let Some(packet) = self.list_sent_reliable_fragment.pop() {
            if packet.delivery.meta.time + now > Duration::from_secs(1000) {
                let meta = MetaDelivery {
                    time: now,
                    address: packet.delivery.meta.address,
                };
                let delivery = ToSend {
                    id: packet.delivery.id,
                    meta,
                };
                let message = Fragment {
                    meta: packet.message.meta,
                    connection_id: packet.message.connection_id,
                    payload: packet.message.payload,
                    index: packet.message.index,
                    total: packet.message.total,
                };
                let result = Packet {
                    delivery,
                    sequence: packet.sequence,
                    ack: packet.ack,
                    message,
                };

                return Some(result);
            }
        }

        None
    }
}

impl<S: ServiceScheduler> Scheduler<S> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            list_scheduled_reliable_data_transfer: Vec::with_capacity(capacity),
            list_scheduled_unreliable_data_transfer: Vec::with_capacity(capacity),
            list_scheduled_reliable_fragment: Vec::with_capacity(capacity),
            list_scheduled_unreliable_fragment: Vec::with_capacity(capacity),
            service: S::new(capacity),
        }
    }

    fn reliable_fragment(&mut self, scheduled: Scheduled<Reliable, Fragment>) {
        self.list_scheduled_reliable_fragment.push(scheduled);
    }

    fn unreliable_fragment(&mut self, scheduled: Scheduled<Unreliable, Fragment>) {
        self.list_scheduled_unreliable_fragment.push(scheduled);
    }

    fn reliable_data_transfer(&mut self, scheduled: Scheduled<Reliable, DataTransfer>) {
        self.list_scheduled_reliable_data_transfer.push(scheduled);
    }

    fn unreliable_data_transfer(&mut self, scheduled: Scheduled<Unreliable, DataTransfer>) {
        self.list_scheduled_unreliable_data_transfer.push(scheduled);
    }

    pub(crate) fn schedule_for_connected_peer(
        &mut self,
        address: SocketAddr,
        payload: Arc<Payload>,
        reliability: ReliabilityType,
        connection_id: ConnectionId,
        packet_id: PacketId,
        time: Duration,
        fragmented: bool,
        fragment_index: usize,
        fragment_total: usize,
    ) {
        if fragment_total > 1 {
            debug!("protocol: scheduling fragment");
            self.schedule_fragment(
                reliability,
                packet_id,
                connection_id,
                payload,
                fragment_index,
                fragment_total,
                time,
                address,
            );
        } else {
            debug!("protocol: scheduling data transfer");
            self.schedule_data_transfer(
                reliability,
                packet_id,
                connection_id,
                payload,
                time,
                address,
            );
        }
    }

    fn schedule_fragment(
        &mut self,
        reliability: ReliabilityType,
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        fragment_index: usize,
        fragment_total: usize,
        time: Duration,
        address: SocketAddr,
    ) {
        match reliability {
            ReliabilityType::Reliable => {
                let scheduled: Scheduled<Reliable, Fragment> = Scheduled::new_reliable_fragment(
                    packet_id,
                    connection_id,
                    payload,
                    fragment_index,
                    fragment_total,
                    time,
                    address,
                );

                self.reliable_fragment(scheduled);
            }
            ReliabilityType::Unreliable => {
                let scheduled: Scheduled<Unreliable, Fragment> = Scheduled::new_unreliable_fragment(
                    packet_id,
                    connection_id,
                    payload,
                    fragment_index,
                    fragment_total,
                    time,
                    address,
                );

                self.unreliable_fragment(scheduled);
            }
        }
    }

    fn schedule_data_transfer(
        &mut self,
        reliability: ReliabilityType,
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        time: Duration,
        address: SocketAddr,
    ) {
        match reliability {
            ReliabilityType::Reliable => {
                let scheduled: Scheduled<Reliable, DataTransfer> =
                    Scheduled::new_reliable_data_transfer(
                        packet_id,
                        connection_id,
                        payload,
                        time,
                        address,
                    );

                self.reliable_data_transfer(scheduled);
            }
            ReliabilityType::Unreliable => {
                let scheduled: Scheduled<Unreliable, DataTransfer> =
                    Scheduled::new_unreliable_data_transfer(
                        packet_id,
                        connection_id,
                        payload,
                        time,
                        address,
                    );

                self.unreliable_data_transfer(scheduled);
            }
        }
    }
}
