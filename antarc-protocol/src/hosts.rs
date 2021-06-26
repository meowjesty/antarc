use std::{
    convert::TryInto,
    io,
    net::SocketAddr,
    time::{Duration, Instant},
};

use log::{debug, error, warn};

use crate::{
    packets::{
        header::{ConnectionAccepted, DataTransfer, Header, Heartbeat},
        payload::Payload,
        received::Received,
        sequence::Sequence,
        Ack, ConnectionId, Footer, Packet, Sent,
    },
    PacketId,
};

/// TODO(alex) 2021-01-29: Think of `Sessions / Channels` when wondering about connections, it helps
/// when trying to figure out how to keep alive a session (connection), how the communication
/// between hosts occur (channels trasnfer packets), and gives more struct names for similar things.

/// TODO(alex) 2021-04-24: There is a difference between many of the client / server host states and
/// how packets are to be handled on each end.
///
/// A client that is in `RequestingConnection` state is someone who sent a connection request packet
/// and will poll and retry sending this same packet until it receives an ack (reliable handshake).
///
/// Meanwhile a server that receives a connection request wants to send back a connection accepted
/// (or denied) packet.
///
/// The `RequestingConnection` seems to make more sense for hosts on the server side, while the
/// client will have a host in the `SendingConnectionRequest` state of sorts.
///
/// This is why writing some client and server APIs right now makes more sense, even if it's just
/// littered with `todo!()`, as I need to get a better understanding on what states belong where.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct Disconnected;

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct RequestingConnection {
    pub attempts: u32,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub struct AwaitingConnectionAck {
    pub attempts: u32,
}

#[derive(Debug, Clone)]
pub struct Connected {
    pub connection_id: ConnectionId,
    /// TODO(alex) [low] 2021-02-13: Do not flood the network, find a way to check if the `rtt` is
    /// increasing due to us flooding the network with packets.
    pub rtt: Duration,
    pub latest_sent_id: PacketId,
    pub latest_sent_time: Duration,

    pub recv_data_transfers: Vec<Packet<Received<DataTransfer>>>,
    pub recv_heartbeats: Vec<Packet<Received<Heartbeat>>>,

    pub sent_data_transfers: Vec<Packet<Sent<DataTransfer>>>,
    pub sent_heartbeats: Vec<Packet<Sent<Heartbeat>>>,
}

#[derive(Debug, Clone)]
pub struct Host<State> {
    pub sequence_tracker: Sequence,
    pub remote_ack_tracker: Ack,
    pub local_ack_tracker: Ack,
    pub address: SocketAddr,
    pub state: State,
}

impl Host<Disconnected> {
    pub fn new_disconnected(address: SocketAddr) -> Self {
        let state = Disconnected;
        Self {
            sequence_tracker: Sequence::default(),
            remote_ack_tracker: 0,
            local_ack_tracker: 0,
            address,
            state,
        }
    }
}

impl Host<Connected> {
    pub fn prepare_data_transfer(
        &self,
        payload: &Payload,
    ) -> (Header<DataTransfer>, Vec<u8>, Footer) {
        let sequence = self.sequence_tracker;
        let ack = self.remote_ack_tracker;
        let connection_id = self.state.connection_id;
        let header = Header::data_transfer(sequence, ack, payload.len().try_into().unwrap());
        let (bytes, footer) = Packet::encode(&payload, &header.info, Some(connection_id));

        (header, bytes, footer)
    }

    pub fn prepare_heartbeat(&self) -> (Header<Heartbeat>, Vec<u8>, Footer) {
        let sequence = self.sequence_tracker;
        let ack = self.remote_ack_tracker;
        let connection_id = self.state.connection_id;
        let header = Header::heartbeat(sequence, ack);
        let payload = Payload::default();
        let (bytes, footer) = Packet::encode(&payload, &header.info, Some(connection_id));

        (header, bytes, footer)
    }

    pub fn after_send(&mut self) {
        debug!("{:#?} after send.", self);
        self.sequence_tracker = Sequence::new(self.sequence_tracker.get() + 1).unwrap();
    }
}

impl Host<RequestingConnection> {
    pub fn starting_connection_request(address: SocketAddr) -> Self {
        let state = RequestingConnection { attempts: 0 };
        Self {
            sequence_tracker: Sequence::default(),
            remote_ack_tracker: 0,
            local_ack_tracker: 0,
            address,
            state,
        }
    }

    pub fn received_connection_request(address: SocketAddr) -> Self {
        let state = RequestingConnection { attempts: 0 };
        Self {
            sequence_tracker: Sequence::default(),
            remote_ack_tracker: 1,
            local_ack_tracker: 0,
            address,
            state,
        }
    }

    pub fn prepare_connection_accepted(
        &self,
        connection_id: ConnectionId,
    ) -> (Header<ConnectionAccepted>, Vec<u8>, Footer) {
        let ack = 1;
        let connection_id = connection_id;
        let header = Header::connection_accepted(ack);
        let payload = Payload::default();

        // NOTE(alex) 2021-05-30: These cannot be cached, as they may become invalid
        // when the time to re-send comes (id, sequence may have increased, causing
        // possible packet duplication).
        let (bytes, footer) = Packet::encode(&payload, &header.info, Some(connection_id));
        (header, bytes, footer)
    }

    pub fn after_send(&mut self) {
        debug!("{:#?} after send.", self);
        self.sequence_tracker = Sequence::new(self.sequence_tracker.get() + 1).unwrap();
    }

    pub fn await_connection(self) -> Host<AwaitingConnectionAck> {
        let state = AwaitingConnectionAck { attempts: 0 };
        let host = Host {
            sequence_tracker: unsafe { Sequence::new_unchecked(self.sequence_tracker.get() + 1) },
            remote_ack_tracker: 1,
            local_ack_tracker: self.local_ack_tracker,
            address: self.address,
            state,
        };

        host
    }
}

impl Host<AwaitingConnectionAck> {
    pub fn connection_accepted(self, connection_id: ConnectionId) -> Host<Connected> {
        let recv_data_transfers = Vec::with_capacity(32);
        let recv_heartbeats = Vec::with_capacity(32);
        let sent_data_transfers = Vec::with_capacity(32);
        let sent_heartbeats = Vec::with_capacity(32);

        let state = Connected {
            connection_id,
            rtt: Duration::default(),
            latest_sent_id: 0,
            latest_sent_time: Duration::default(),
            recv_data_transfers,
            recv_heartbeats,
            sent_data_transfers,
            sent_heartbeats,
        };
        let host = Host {
            sequence_tracker: unsafe { Sequence::new_unchecked(self.sequence_tracker.get() + 1) },
            remote_ack_tracker: 1,
            local_ack_tracker: self.local_ack_tracker,
            address: self.address,
            state,
        };

        host
    }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub struct Address(pub SocketAddr);

pub const RESEND_TIMEOUT_THRESHOLD: Duration = Duration::from_millis(500);
pub const CONNECTION_TIMEOUT_THRESHOLD: Duration = Duration::new(2, 0);