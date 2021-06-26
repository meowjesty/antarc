use core::mem;
use std::marker::PhantomData;

use crate::{
    packets::{Ack, StatusCode},
    sequence::Sequence,
    ProtocolId,
};

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct HeaderInfo {
    pub sequence: Sequence,
    pub ack: Ack,
    pub past_acks: u16,
    pub status_code: StatusCode,
    pub payload_length: u16,
}

// TODO(alex) 2021-06-13 [high]: Is there any reason to keep the list of all payloads received/sent?
// When the user schedules a packet, we keep the payload as part of the event, so we ping-pong the
// payload until it's properly sent, or the packet is cancelled.
// On the receiver side, we don't care at all about the payload, as there is no failure point, we
// hold it until the user retrieves all payloads and that's it.
// Think dropping it from here makes perfect sense.

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct Generic;

#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct Header<Kind> {
    pub info: HeaderInfo,
    pub marker: PhantomData<Kind>,
}

/// TODO(alex) 2021-01-31: I don't want methods in the `Header`, the `kind` will be defined by the
/// `impl Packet<Kind>` of each packet type, but this will have a ton of code duplication, as the
/// only difference will be the `Header.kind` field. Should these be macros, that create
/// `impl Header<Kind>` for each kind of packet as well?
///
/// ADD(alex): 2021-02-03: Code duplication will be a minor issue with the state approach I'm using,
/// just have an `impl` block for the relevant meta-state part of each struct that feeds the

/// `Header.kind` field during encoding / decoding.

pub const ENCODED_SIZE: usize = mem::size_of::<Sequence>()
    + mem::size_of::<Ack>()
    + mem::size_of::<u16>()
    + mem::size_of::<StatusCode>()
    + mem::size_of::<u16>()
    + mem::size_of::<ProtocolId>();
