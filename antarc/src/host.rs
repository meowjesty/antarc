use std::{
    marker::PhantomData,
    net::SocketAddr,
    num::{NonZeroU16, NonZeroU32},
};

use crate::{
    packet::{Acked, Header, Internal, Packet, Received, Retrieved, Sent, ToSend},
    CONNECTION_REQUEST,
};

/// TODO(alex) 2021-01-29: Think of `Sessions / Channels` when wondering about connections, it helps
/// when trying to figure out how to keep alive a session (connection), how the communication
/// between hosts occur (channels trasnfer packets), and gives more struct names for similar things.
#[derive(Debug)]
pub struct Connecting;

#[derive(Debug)]
pub struct Connected;

#[derive(Debug)]
pub struct Disconnected;

#[derive(Debug)]
pub(crate) struct Host<ConnectionState> {
    pub(crate) address: SocketAddr,
    pub(crate) sequence: NonZeroU32,
    /// TODO(alex) 2021-02-13: Do not flood the network, find a way to check if the `rtt` is
    /// increasing due to us flooding the network with packets.
    pub(crate) rtt: u128,
    pub(crate) received_list: Vec<Packet<Received>>,
    pub(crate) retrieved: Vec<Packet<Retrieved>>,
    pub(crate) internals: Vec<Packet<Internal>>,
    pub(crate) send_queue: Vec<Packet<ToSend>>,
    pub(crate) sent: Vec<Packet<Sent>>,
    pub(crate) acked: Vec<Packet<Acked>>,
    _phantom: PhantomData<ConnectionState>,
}

impl Host<Disconnected> {
    /// TODO(alex) 2021-02-07: Is it possible to create a `Host` in any other state? Or should it
    /// always start in disconnected mode?
    pub(crate) fn new(address: SocketAddr) -> Host<Disconnected> {
        let state = Disconnected;
        let host = Host {
            address,
            sequence: unsafe { NonZeroU32::new_unchecked(1) },
            rtt: 0,
            received_list: Vec::with_capacity(32),
            retrieved: Vec::with_capacity(32),
            internals: Vec::with_capacity(32),
            send_queue: Vec::with_capacity(32),
            sent: Vec::with_capacity(32),
            acked: Vec::with_capacity(32),
            _phantom: PhantomData::default(),
        };

        host
    }

    pub(crate) fn into_connecting(mut self, connection_id: NonZeroU16) -> Host<Connecting> {
        let connection_header = Header {
            connection_id,
            sequence: unsafe { NonZeroU32::new_unchecked(1) },
            ack: 0,
            past_acks: 0,
            kind: CONNECTION_REQUEST,
        };
        let connection_request = Packet::<ToSend>::new(connection_header, vec![0; 10]);
        // TODO(alex) 2021-02-17: This should be marked as priority and reliable, we can't move on
        // until the connection is estabilished, and the user cannot be able to put packets into
        // the `send_queue` until the protocol is done handling it.
        self.send_queue.push(connection_request);

        let host = Host {
            address: self.address,
            sequence: self.sequence,
            rtt: self.rtt,
            received_list: self.received_list,
            retrieved: self.retrieved,
            internals: self.internals,
            send_queue: self.send_queue,
            sent: self.sent,
            acked: self.acked,
            _phantom: PhantomData::default(),
        };

        host
    }
}

impl Host<Connecting> {
    pub(crate) fn into_connected(mut self) -> Host<Connected> {
        let host = Host {
            address: self.address,
            sequence: self.sequence,
            rtt: self.rtt,
            received_list: self.received_list,
            retrieved: self.retrieved,
            internals: self.internals,
            send_queue: self.send_queue,
            sent: self.sent,
            acked: self.acked,
            _phantom: PhantomData::default(),
        };

        host
    }
}
