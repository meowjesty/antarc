use core::mem;
use std::marker::PhantomData;

use crate::{
    packets::{Ack},
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


#[derive(Debug, PartialEq, Clone, Eq, Hash, PartialOrd)]
pub struct Header<Kind> {
    pub info: HeaderInfo,
    pub marker: PhantomData<Kind>,
}

pub const ENCODED_SIZE: usize = mem::size_of::<Sequence>()
    + mem::size_of::<Ack>()
    + mem::size_of::<u16>()
    + mem::size_of::<StatusCode>()
    + mem::size_of::<u16>()
    + mem::size_of::<ProtocolId>();
