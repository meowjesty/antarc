#![feature(duration_consts_2)]
#![feature(drain_filter)]
#![feature(hash_drain_filter)]
#![feature(nonzero_ops)]
// #![feature(const_generics)]
// #![feature(const_try)]

use core::mem::size_of;
use std::{
    net::SocketAddr,
    num::NonZeroU32,
    sync::Arc,
    time::{Duration, Instant},
};

use log::debug;
use packets::*;

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

pub trait Service {}

/// NOTE(alex): `Service` may be either a `Client` or a `Server`.
#[derive(Debug)]
pub struct Protocol<S: Service> {
    pub packet_id_tracker: PacketId,
    pub timer: Instant,
    pub service: S,
    pub receiver_pipe: Vec<RawPacket<S>>,
}

impl<S> Protocol<S>
where
    S: Service,
{
    pub fn cancel_packet(&mut self, _packet_id: PacketId, _reliability: ReliabilityType) -> bool {
        todo!()
    }
}

pub(crate) trait ServiceScheduler {
    fn new(capacity: usize) -> Self;
}

#[derive(Debug)]
pub(crate) struct Scheduler<S: ServiceScheduler> {
    list_scheduled_reliable_data_transfer: Vec<Scheduled<Reliable, DataTransfer>>,
    list_scheduled_unreliable_data_transfer: Vec<Scheduled<Unreliable, DataTransfer>>,
    list_scheduled_reliable_fragment: Vec<Scheduled<Reliable, Fragment>>,
    list_scheduled_unreliable_fragment: Vec<Scheduled<Unreliable, Fragment>>,

    service: S,
}

pub(crate) trait ServiceReliability {
    fn new(capacity: usize) -> Self;
}

#[derive(Debug)]
pub(crate) struct ReliabilityHandler<S: ServiceReliability> {
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

    pub(crate) fn resend_reliable_data_transfer(
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

    pub(crate) fn resend_reliable_fragment(
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
        if fragmented {
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
