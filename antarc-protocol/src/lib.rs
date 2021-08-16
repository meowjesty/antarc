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
    time::{Duration, Instant},
};

use events::*;
use packets::*;

pub mod client;
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

pub(crate) trait ServiceScheduler {
    fn new(capacity: usize) -> Self;
}

/// NOTE(alex): `Service` may be either a `Client` or a `Server`.
#[derive(Debug)]
pub struct Protocol<S: Service> {
    pub packet_id_tracker: PacketId,
    pub timer: Instant,
    pub service: S,
    pub events: EventSystem,
    pub receiver_pipe: Vec<RawPacket<S>>,
}

impl<S> Protocol<S>
where
    S: Service,
{
    pub fn cancel_packet(&mut self, _packet_id: PacketId) -> bool {
        todo!()
    }
}

#[derive(Debug)]
pub struct EventSystem {
    pub receiver: Vec<ReceiverEvent>,
    pub reliable_sent: Vec<ReliableSentEvent>,
}

impl EventSystem {
    pub fn new() -> Self {
        let receiver = Vec::with_capacity(1024);
        let reliable_sent = Vec::with_capacity(1024);

        Self {
            receiver,
            reliable_sent,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Scheduler<S: ServiceScheduler> {
    list_scheduled_reliable_data_transfer: Vec<Scheduled<Reliable, DataTransfer>>,
    list_scheduled_unreliable_data_transfer: Vec<Scheduled<Unreliable, DataTransfer>>,
    list_scheduled_reliable_fragment: Vec<Scheduled<Reliable, Fragment>>,
    list_scheduled_unreliable_fragment: Vec<Scheduled<Unreliable, Fragment>>,

    service: S,
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

    pub(crate) fn schedule_fragment(
        &mut self,
        reliability: ReliabilityType,
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Payload,
        fragment_index: usize,
        fragment_total: usize,
        time: Duration,
        address: SocketAddr,
    ) {
        // TODO(alex) [mid] 2021-08-08: This and the data transfer part are
        // finnicky, if I call the wrong `new` function, we could end up with both
        // sides creating the same `ScheduleEvent`.
        //
        // Maybe one simple way of solving this would be to put the `Reliable` and
        // `Unreliable` types inside the `ReliabilityType::Reliable(Reliable)` for
        // example, and pass it into the `new` functions.
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

    pub(crate) fn schedule_data_transfer(
        &mut self,
        reliability: ReliabilityType,
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Payload,
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
