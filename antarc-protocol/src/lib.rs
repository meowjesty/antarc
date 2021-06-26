#![feature(duration_consts_2)]
#![feature(drain_filter)]

use core::mem::size_of;
use std::{
    convert::TryInto,
    num::NonZeroU32,
    time::{Duration, Instant},
    vec::Drain,
};

use connection::ConnectionSystem;
use events::{ProtocolError, SenderEvent};
use packets::{raw::RawPacket, ConnectionId};

use crate::{
    controls::{connection_request::ConnectionRequest, data_transfer::DataTransfer},
    packets::{received::Received, Packet},
};

pub mod connection;
pub mod controls;
pub mod events;
pub mod header;
pub mod hosts;
pub mod packets;
pub mod payload;
pub mod sequence;

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

pub type PacketId = u64;
pub type ProtocolId = NonZeroU32;

pub const PROTOCOL_ID: ProtocolId = unsafe { NonZeroU32::new_unchecked(0xbabedad) };
pub const PROTOCOL_ID_BYTES: [u8; size_of::<ProtocolId>()] = PROTOCOL_ID.get().to_be_bytes();

#[derive(Debug)]
pub struct Server {
    pub last_antarc_schedule_check: Duration,
    pub connection_id_tracker: ConnectionId,
}

#[derive(Debug)]
pub struct Client {
    pub last_sent_time: Duration,
}

#[derive(Debug)]
pub struct Protocol<T> {
    pub timer: Instant,
    pub event_system: EventSystem,
    pub retrievable: Vec<(ConnectionId, Vec<u8>)>,
    pub kind: T,
    pub connection_system: ConnectionSystem,
}

impl Protocol<Server> {
    pub fn new_server() -> Self {
        todo!()
    }

    pub fn on_received(&mut self, raw_packet: RawPacket) -> Result<(), ProtocolError> {
        let partial_packet = raw_packet.decode(self.connection_system.packet_id_tracker)?;
        let source = partial_packet.address;

        if let Some(host) = self
            .connection_system
            .requesting_connection
            .drain_filter(|host| host.address == source)
            .next()
        {
            match partial_packet.try_into()? {}
        } else if let Some(host) = self
            .connection_system
            .awaiting_connection_ack
            .drain_filter(|host| host.address == source)
            .next()
        {
            match partial_packet.try_into()? {}
        } else if let Some(host) = self
            .connection_system
            .connected
            .drain_filter(|host| host.address == source)
            .next()
        {
            match partial_packet.try_into()? {}
        } else {
            match partial_packet.try_into()? {}
        }

        todo!()
    }
}

impl Protocol<Client> {
    pub fn new_client() -> Self {
        todo!()
    }

    pub fn on_received(&mut self, raw_packet: RawPacket) -> Result<(), ProtocolError> {
        let partial_packet = raw_packet.decode(self.connection_system.packet_id_tracker)?;
        let source = partial_packet.address;

        if let Some(host) = self
            .connection_system
            .requesting_connection
            .drain_filter(|host| host.address == source)
            .next()
        {
            return Err(todo!());
        } else if let Some(host) = self
            .connection_system
            .awaiting_connection_ack
            .drain_filter(|host| host.address == source)
            .next()
        {
            match partial_packet.try_into()? {}
        } else if let Some(host) = self
            .connection_system
            .connected
            .drain_filter(|host| host.address == source)
            .next()
        {
            match partial_packet.try_into()? {}
        } else {
            match partial_packet.try_into()? {}
        }

        todo!()
    }
}

impl<T> Protocol<T> {
    pub fn retrieve(&mut self) -> Drain<(ConnectionId, Vec<u8>)> {
        self.retrievable.drain(..)
    }

    pub fn cancel_packet(&mut self, packet_id: PacketId) -> bool {
        let cancelled_packet = self
            .event_system
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
}

impl EventSystem {
    pub fn new() -> Self {
        let sender = Vec::with_capacity(1024);

        Self { sender }
    }
}
