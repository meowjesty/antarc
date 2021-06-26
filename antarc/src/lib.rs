#![allow(unused_imports)]
#![feature(const_panic)]
#![feature(write_all_vectored)]
#![feature(bool_to_option)]
#![feature(format_args_capture)]
// https://github.com/rust-lang/rust/issues/66753#issuecomment-644285006
// #![feature(const_precise_live_drops)]
#![feature(const_fn_floating_point_arithmetic)]
#![feature(duration_consts_2)]
#![feature(new_uninit)]
#![feature(drain_filter)]
#![feature(hash_drain_filter)]

use core::mem::size_of;
use std::{
    any::Any,
    num::{NonZeroU16, NonZeroU32, NonZeroU8},
    time::Duration,
};

// pub mod client;
// pub mod receiver;
// pub mod sender;
// pub mod server;
pub mod net;

/// An exponential moving average - EMA is a type of moving average that places a greater weight and
/// significance on the most recent data points.
///
/// The weighting for each **older** datum **decreases** exponentially, never reaching zero.
#[allow(dead_code)]
pub const fn exponential_moving_average(new_value: Duration, old_value: Duration) -> Duration {
    let weight = 0.1;
    // TODO(alex): `const fn` complains without `saturating_add`, maybe `+` operator is not marked
    // as const fn for `Duration`?
    let result = new_value
        .mul_f64(weight)
        .saturating_add(old_value.mul_f64(1.0 - weight));
    result
}

// TODO(alex) 2021-02-27 There needs to be a check on the send schedule, so that we don't keep
// sending and acking packets of the same host/peer, otherwise we might end up in a situation where
// other peers never get a message back from the server.

/// Protocol id type alias that identifies a packet as part of the protocol.
/// This won't be sent to a remote host, but it's used in the CRC32 calculation, it acts as a check
/// between hosts that the CRC32 is correct, as the receiver will take the CRC32 out of the packet,
/// insert the `ProtocolId`, calculate the packet CRC32 and check against what they received.

/// The whole buffer + `MARKER` + `END_OF_PACKET` markers.
pub const MTU_LENGTH: usize = 1500;

// TODO(alex) 2021-01-24: How does send / receive works?
// - Packet is sent with data / client replies with either data or just ack;
// - Server receives the ack, but has no data to send, then send a heartbeat to test the connection;
// TODO(alex) 2021-01-24: Add some form of expiration for a packet (stop trying to resend it, and
// remove it from the list of `to_send` packets).
