#![feature(duration_consts_2)]
#![feature(drain_filter)]
#![feature(hash_drain_filter)]

use core::mem::size_of;
use std::{num::NonZeroU32, time::Instant, vec::Drain};

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
    pub service: Service,
    pub events: EventSystem,
    pub receiver_pipe: Vec<RawPacket>,
}

impl<Service> Protocol<Service> {
    pub fn cancel_packet(&mut self, packet_id: PacketId) -> bool {
        todo!()
    }
}

#[derive(Debug)]
pub struct EventSystem {
    pub receiver: Vec<ReceiverEvent>,
    pub scheduler: Vec<ScheduleEvent>,
    pub api: Vec<AntarcEvent>,
}

impl EventSystem {
    pub fn new() -> Self {
        let receiver = Vec::with_capacity(1024);
        let api = Vec::with_capacity(1024);
        let scheduler = Vec::with_capacity(1024);

        Self {
            receiver,
            api,
            scheduler,
        }
    }
}
