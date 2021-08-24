use core::convert::TryFrom;
use std::fmt::Display;

use super::{PACKET_TYPE_SENTINEL_END, PACKET_TYPE_SENTINEL_START};
use crate::errors::ProtocolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
#[repr(transparent)]
// TODO(alex) [low] 2021-08-24: Would be nice to have these, but they're libcore and probably not
// supposed to be used elsewhere.
// #[rustc_layout_scalar_valid_range_end(1)]
// #[rustc_nonnull_optimization_guaranteed]
pub struct PacketType(u8);

impl PacketType {
    #[allow(unused_unsafe)]
    #[inline]
    pub const unsafe fn new_unchecked(n: u8) -> Self {
        unsafe { Self(n) }
    }

    #[allow(unused_unsafe)]
    #[inline]
    pub const fn new(n: u8) -> Option<Self> {
        if n > PACKET_TYPE_SENTINEL_START && n < PACKET_TYPE_SENTINEL_END {
            Some(unsafe { Self(n) })
        } else {
            None
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    pub const fn to_be_bytes(self) -> [u8; 1] {
        self.0.to_be_bytes()
    }

    pub const fn from_be_bytes(bytes: [u8; 1]) -> u8 {
        u8::from_be_bytes(bytes)
    }
}

impl From<PacketType> for u8 {
    fn from(packet_type: PacketType) -> Self {
        packet_type.0
    }
}

impl TryFrom<u8> for PacketType {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        PacketType::new(value).ok_or(ProtocolError::PacketTypeConversion(value))
    }
}

impl Display for PacketType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}", self.0))
    }
}
