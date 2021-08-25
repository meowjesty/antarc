use core::{mem::size_of, ops::Range, time::Duration};
use std::{
    convert::TryFrom,
    marker::PhantomData,
    net::SocketAddr,
    num::{NonZeroU16, NonZeroU32},
    sync::Arc,
};

use crc32fast::Hasher;
use log::*;

use self::{decode::*, packet_type::*};
use crate::{client::*, errors::*, server::*, *};

pub mod decode;
pub mod delivery;
pub mod encode;
pub mod message;
pub mod packet_type;
pub mod raw;
pub mod scheduled;

pub type PacketId = u64;
pub type Sequence = NonZeroU32;
pub type ConnectionId = NonZeroU16;
pub type Ack = u32;
pub type Payload = Vec<u8>;

pub const MAX_FRAGMENT_SIZE: usize = 1500;

pub const PACKET_TYPE_RANGE: Range<u8> = (PACKET_TYPE_SENTINEL_START + 1)..PACKET_TYPE_SENTINEL_END;
pub const PACKET_TYPE_SENTINEL_START: u8 = 0x0;
pub const CONNECTION_REQUEST: PacketType = unsafe { PacketType::new_unchecked(0x1) };
pub const CONNECTION_ACCEPTED: PacketType = unsafe { PacketType::new_unchecked(0x2) };
pub const DATA_TRANSFER: PacketType = unsafe { PacketType::new_unchecked(0x3) };
pub const FRAGMENT: PacketType = unsafe { PacketType::new_unchecked(0x4) };
pub const HEARTBEAT: PacketType = unsafe { PacketType::new_unchecked(0x5) };
pub const PACKET_TYPE_SENTINEL_END: u8 = 0x6;

pub trait Delivery {}

pub trait Message {
    const PACKET_TYPE: PacketType;
    const PACKET_TYPE_BYTES: [u8; 1];
}

pub trait Encoder {
    const HEADER_SIZE: usize;
    fn encoded(&self) -> Vec<u8>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Packet<D, M>
where
    D: Delivery,
    M: Message + Encoder,
{
    pub delivery: D,
    pub sequence: Sequence,
    pub ack: Ack,
    pub message: M,
}

impl<M> Packet<ToSend, M>
where
    M: Message + Encoder,
{
    pub fn sent(self, time: Duration, ttl: Duration) -> Packet<Sent, M> {
        let packet = Packet {
            delivery: Sent {
                id: self.delivery.id,
                ttl,
                meta: MetaDelivery {
                    time,
                    address: self.delivery.meta.address,
                },
            },
            sequence: self.sequence,
            ack: self.ack,
            message: self.message,
        };

        packet
    }

    pub fn as_raw<T>(&self) -> RawPacket<T> {
        let sequence_bytes = self.sequence.get().to_be_bytes();
        let ack_bytes = self.ack.to_be_bytes();

        let encoded_message = self.message.encoded();

        let mut hasher = Hasher::new();
        let mut bytes = [
            PROTOCOL_ID_BYTES.as_ref(),
            sequence_bytes.as_ref(),
            ack_bytes.as_ref(),
            &encoded_message,
        ]
        .concat();

        hasher.update(&bytes);
        let crc32 = hasher.finalize();
        debug_assert!(crc32 != 0);

        bytes.append(&mut crc32.to_be_bytes().to_vec());

        let packet_bytes = bytes[size_of::<ProtocolId>()..].to_vec();
        let raw_packet = RawPacket {
            address: self.delivery.meta.address,
            bytes: packet_bytes,
            phantom: PhantomData::default(),
        };

        // debug_assert_eq!(self, raw_packet.decode()?)

        raw_packet
    }
}
