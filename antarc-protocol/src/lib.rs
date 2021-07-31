#![feature(duration_consts_2)]
#![feature(drain_filter)]
#![feature(hash_drain_filter)]

use core::mem::size_of;
use std::{num::NonZeroU32, time::Instant, vec::Drain};

use events::{AntarcEvent, ReceiverEvent, SenderEvent};
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
        let bytes_arr: &[u8; size_of::<$kind>()] = $buffer[$start..end].try_into().unwrap();
        let val = $kind::from_be_bytes(*bytes_arr);
        $start = end;
        val
    }};
}

pub type ProtocolId = NonZeroU32;

pub const PROTOCOL_ID: ProtocolId = unsafe { NonZeroU32::new_unchecked(0xbabedad) };
pub const PROTOCOL_ID_BYTES: [u8; size_of::<ProtocolId>()] = PROTOCOL_ID.get().to_be_bytes();

/// NOTE(alex): `Service` may be either a `Client` or a `Server`.
#[derive(Debug)]
pub struct Protocol<Service> {
    pub timer: Instant,
    pub retrievable: Vec<(ConnectionId, Vec<u8>)>,
    pub service: Service,
    pub events: EventSystem,
    pub scheduler_pipe: Vec<Scheduled>,
    pub receiver_pipe: Vec<RawPacket>,
}

impl<Service> Protocol<Service> {
    pub fn retrieve(&mut self) -> Drain<(ConnectionId, Vec<u8>)> {
        self.retrievable.drain(..)
    }

    pub fn cancel_packet(&mut self, packet_id: PacketId) -> bool {
        let cancelled_packet = self
            .events
            .sender
            .drain_filter(|event| match event {
                SenderEvent::ScheduledDataTransfer { packet, .. } => packet.id == packet_id,
                _ => false,
            })
            .next()
            .is_some();

        cancelled_packet
    }
}

#[derive(Debug)]
pub struct EventSystem {
    pub sender: Vec<SenderEvent>,
    pub receiver: Vec<ReceiverEvent>,
    pub api: Vec<AntarcEvent>,
}

impl EventSystem {
    pub fn new() -> Self {
        let sender = Vec::with_capacity(1024);
        let receiver = Vec::with_capacity(1024);
        let api = Vec::with_capacity(1024);

        Self {
            sender,
            receiver,
            api,
        }
    }
}
