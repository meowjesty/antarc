use std::{
    convert::TryInto,
    io,
    net::SocketAddr,
    time::{Duration, Instant},
};

use log::{debug, error, warn};
use mio::net::UdpSocket;

use crate::{
    events::{AntarcError, FailureEvent},
    net::server::PacketId,
    packet::{
        header::{ConnectionAccepted, DataTransfer, Header, Heartbeat},
        payload::{self, Payload},
        queued::Queued,
        received::Received,
        sequence::Sequence,
        Ack, ConnectionId, Footer, Packet, Sent,
    },
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
pub(crate) struct Disconnected;

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub(crate) struct RequestingConnection {
    pub(crate) attempts: u32,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub(crate) struct SendingConnectionRequest {
    pub(crate) attempts: u32,
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub(crate) struct AwaitingConnectionAck {
    pub(crate) attempts: u32,
    pub(crate) connection_id: ConnectionId,
    pub(crate) last_sent: PacketId,
}

#[derive(Debug, Clone)]
pub(crate) struct AwaitingConnectionResponse {
    pub(crate) attempts: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct AckingConnection {
    attempts: u32,
    pub(crate) connection_id: ConnectionId,
}

#[derive(Debug, Clone)]
pub(crate) struct Connected {
    pub(crate) connection_id: ConnectionId,
    /// TODO(alex) [low] 2021-02-13: Do not flood the network, find a way to check if the `rtt` is
    /// increasing due to us flooding the network with packets.
    pub(crate) rtt: Duration,
    pub(crate) last_sent: PacketId,
    pub(crate) time_last_sent: Duration,
}

#[derive(Debug, Clone)]
pub(crate) struct Host<State> {
    pub(crate) sequence_tracker: Sequence,
    pub(crate) remote_ack_tracker: Ack,
    pub(crate) local_ack_tracker: Ack,
    pub(crate) address: SocketAddr,
    // TODO(alex) [mid] 2021-05-26: `Received` should not contain the payload, let's put the
    // payload in a `HashMap<ConnectionId, Vec<Payload>>` in the server/client, to ease the
    // `retrieve` impl.
    pub(crate) received: Vec<Packet<Received<DataTransfer>>>,
    pub(crate) state: State,
}

impl Host<Disconnected> {
    pub(crate) fn new_disconnected(address: SocketAddr) -> Self {
        let state = Disconnected;
        Self {
            sequence_tracker: Sequence::default(),
            remote_ack_tracker: 0,
            local_ack_tracker: 0,
            address,
            received: Vec::with_capacity(32),
            state,
        }
    }
}

impl Host<Generic> {
    pub(crate) fn new_generic(address: SocketAddr) -> Self {
        let state = Generic {
            state: HostState::Disconnected,
        };

        Self {
            sequence_tracker: Sequence::default(),
            remote_ack_tracker: 0,
            local_ack_tracker: 0,
            address,
            received: Vec::with_capacity(32),
            state,
        }
    }

    pub(crate) fn prepare_data_transfer(
        &self,
        payload: Payload,
    ) -> (Header<DataTransfer>, Vec<u8>, Footer) {
        if let HostState::Connected { info } = &self.state.state {
            let sequence = self.sequence_tracker;
            let ack = self.remote_ack_tracker;
            let connection_id = info.connection_id;
            let header = Header::data_transfer(sequence, ack, payload);
            let (bytes, footer) =
                Packet::encode(&header.kind.payload, &header.info, Some(connection_id));

            (header, bytes, footer)
        } else {
            panic!("Host invalid state {:#?}", self);
        }
    }

    pub(crate) fn prepare_heartbeat(&self) -> (Header<Heartbeat>, Vec<u8>, Footer) {
        if let HostState::Connected { info } = &self.state.state {
            let sequence = self.sequence_tracker;
            let ack = self.remote_ack_tracker;
            let connection_id = info.connection_id;
            let header = Header::heartbeat(sequence, ack);
            let payload = Payload::default();
            let (bytes, footer) = Packet::encode(&payload, &header.info, Some(connection_id));

            (header, bytes, footer)
        } else {
            panic!("Host invalid state {:#?}", self);
        }
    }

    pub(crate) fn new_local() -> Self {
        Self::new_generic("127.0.0.1:7777".parse().unwrap())
    }

    pub(crate) fn disconnected(&self) -> bool {
        match self.state.state {
            HostState::Disconnected => true,
            _ => false,
        }
    }

    pub(crate) fn after_send(&mut self) {
        self.sequence_tracker = Sequence::new(self.sequence_tracker.get() + 1).unwrap();
    }
}

impl Host<Connected> {
    pub(crate) fn prepare_data_transfer(
        &self,
        payload: Payload,
    ) -> (Header<DataTransfer>, Vec<u8>, Footer) {
        let sequence = self.sequence_tracker;
        let ack = self.remote_ack_tracker;
        let connection_id = self.state.connection_id;
        let header = Header::data_transfer(sequence, ack, payload);
        let (bytes, footer) =
            Packet::encode(&header.kind.payload, &header.info, Some(connection_id));

        (header, bytes, footer)
    }

    pub(crate) fn prepare_heartbeat(&self) -> (Header<Heartbeat>, Vec<u8>, Footer) {
        let sequence = self.sequence_tracker;
        let ack = self.remote_ack_tracker;
        let connection_id = self.state.connection_id;
        let header = Header::heartbeat(sequence, ack);
        let payload = Payload::default();
        let (bytes, footer) = Packet::encode(&payload, &header.info, Some(connection_id));

        (header, bytes, footer)
    }

    pub(crate) fn after_send(&mut self) {
        debug!("{:#?} after send.", self);
        self.sequence_tracker = Sequence::new(self.sequence_tracker.get() + 1).unwrap();
    }
}

impl Host<RequestingConnection> {
    pub(crate) fn new_requesting_connection(address: SocketAddr) -> Self {
        let state = RequestingConnection { attempts: 0 };
        Self {
            sequence_tracker: Sequence::default(),
            remote_ack_tracker: 0,
            local_ack_tracker: 0,
            address,
            received: Vec::with_capacity(32),
            state,
        }
    }

    pub(crate) fn prepare_connection_accepted(
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

    pub(crate) fn after_send(&mut self) {
        debug!("{:#?} after send.", self);
        self.sequence_tracker = Sequence::new(self.sequence_tracker.get() + 1).unwrap();
    }

    pub(crate) fn await_connection(
        self,
        connection_id: ConnectionId,
    ) -> Host<AwaitingConnectionAck> {
        let state = AwaitingConnectionAck {
            attempts: 0,
            connection_id,
            last_sent: 1,
        };
        let host = Host {
            sequence_tracker: unsafe { Sequence::new_unchecked(self.sequence_tracker.get() + 1) },
            remote_ack_tracker: 1,
            local_ack_tracker: self.local_ack_tracker,
            address: self.address,
            received: self.received,
            state,
        };

        host
    }
}

impl Host<AwaitingConnectionAck> {
    pub(crate) fn connection_accepted(self) -> Host<Connected> {
        let state = Connected {
            connection_id: self.state.connection_id,
            rtt: Duration::default(),
            last_sent: self.state.last_sent,
            time_last_sent: Duration::default(),
        };
        let host = Host {
            sequence_tracker: unsafe { Sequence::new_unchecked(self.sequence_tracker.get() + 1) },
            remote_ack_tracker: 1,
            local_ack_tracker: self.local_ack_tracker,
            address: self.address,
            received: self.received,
            state,
        };

        host
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Generic {
    // TODO(alex) 2021-05-17: This name is horrible, the naming conventions used here are kinda
    // confusing in general, `host.rs` file warrants a big renaming.
    pub(crate) state: HostState,
}

// TODO(alex) 2021-05-17: Should I reverse this with `HostInfo`? We could have a struct `Host` that
// takes an enum `HostState` with these additional fields, this will simplify passing down hosts
// between events and so on.
#[derive(Debug, Clone)]
pub(crate) enum HostState {
    Disconnected,
    RequestingConnection { info: RequestingConnection },
    AwaitingConnectionResponse { info: AwaitingConnectionResponse },
    Connected { info: Connected },
}

impl HostState {
    pub(crate) fn connection_id(&self) -> Option<ConnectionId> {
        match self {
            HostState::Connected { info } => Some(info.connection_id),
            _ => None,
        }
    }
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub(crate) struct Address(pub(crate) SocketAddr);

pub(crate) const RESEND_TIMEOUT_THRESHOLD: Duration = Duration::from_millis(500);
pub(crate) const CONNECTION_TIMEOUT_THRESHOLD: Duration = Duration::new(2, 0);
