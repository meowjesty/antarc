use std::{
    convert::TryInto,
    net::SocketAddr,
    time::{Duration, Instant},
};

use hecs::Entity;
use log::error;

use crate::packet::{Acked, ConnectionId, Packet, PacketKind, Queued, Received, Retrieved, Sent};

pub(crate) mod requesting_connection;
pub(crate) mod sending_connection_request;

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
pub(crate) struct StateEnteredTime(pub(crate) Duration);

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
    /// TODO(alex) 2021-02-13: Do not flood the network, find a way to check if the `rtt` is
    /// increasing due to us flooding the network with packets.
    pub(crate) rtt: Duration,
}

#[derive(Debug, PartialEq)]
pub(crate) struct LatestReceived {
    pub(crate) packet_id: Entity,
}

#[derive(Debug, PartialEq, PartialOrd)]
pub(crate) struct LatestSent {
    pub(crate) packet_id: Entity,
}

#[derive(Debug, PartialEq, PartialOrd, Clone)]
pub(crate) struct Address(pub(crate) SocketAddr);

pub(crate) const RESEND_TIMEOUT_THRESHOLD: Duration = Duration::from_millis(500);
pub(crate) const CONNECTION_TIMEOUT_THRESHOLD: Duration = Duration::new(2, 0);

type PacketList = Vec<Packet>;

// TODO(alex): This struct has no business existing, as we only want hosts to be in one of the
// possible states, and most of the info here is not valid anyway. The tracker fields don't work,
// they could add inconsistencies between the latest packet received and the ack tracker, for
// example, so caching these values is a waste and potentially dangerous. The value that makes sense
// holding is the `connection_id`, as this won't change until the host is in a different state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HostInfo {
    pub(crate) address: SocketAddr,
    /// NOTE(alex) 2021-03-02: `sequence` is incremented only after a packet is successfully sent
    /// (`Packet<Sent>`), this is done to prevent remote `Host`s from thinking that some packets
    /// were lost, even in the case of them never being sent.
    pub(crate) queued_packets: PacketList,
    pub(crate) sent_packets: PacketList,
    pub(crate) acked_packets: PacketList,
    pub(crate) received_packets: PacketList,
    pub(crate) retrieved_packets: PacketList,
    pub(crate) rtt_tracker: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum HostState {
    Disconnected,
    RequestingConnection,
    Connected,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Host {
    pub(crate) info: HostInfo,
    pub(crate) state: HostState,
}

impl Host {
    pub(crate) fn disconnected(address: SocketAddr) -> Self {
        let state = HostState::Disconnected;
        let info = HostInfo {
            address,
            queued_packets: Vec::with_capacity(32),
            sent_packets: Vec::with_capacity(32),
            acked_packets: Vec::with_capacity(32),
            received_packets: Vec::with_capacity(32),
            retrieved_packets: Vec::with_capacity(32),
            rtt_tracker: Duration::default(),
        };

        Self { info, state }
    }

    pub(crate) fn on_received(&mut self, bytes: &[u8], timer: &Instant) {
        let packet = Packet::decode(bytes, timer).unwrap();

        for sent in self
            .info
            .sent_packets
            .drain_filter(|sent| sent.header.sequence.get() == packet.header.ack)
        {
            // let acked = sent.to_acked(timer.elapsed());
            // self.info.acked_packets.push(acked);
        }

        // TODO(alex) 2021-05-13: Do the same `drain_filter` for `past_acks`.
        for sent in self.info.sent_packets.drain_filter(|sent| false) {
            todo!()
        }

        match packet.kind {
            PacketKind::ConnectionRequest if self.state == HostState::Disconnected => {
                self.state = HostState::RequestingConnection;
            }
            PacketKind::ConnectionDenied => {
                self.state = HostState::Disconnected;
            }
            PacketKind::ConnectionAccepted => {
                self.state = HostState::Connected;
            }
            PacketKind::DataTransfer => {
                todo!()
            }
            PacketKind::Heartbeat => {
                todo!()
            }
            invalid => {
                error!(
                    "Packet is {:?}, but host is in an invalid state {:?}.",
                    invalid, self
                );
                todo!();
            }
        }

        self.info.received_packets.push(packet);
    }
}

impl Default for Host {
    fn default() -> Self {
        let state = HostState::Disconnected;
        let info = HostInfo {
            address: "127.0.0.1:7777".parse().unwrap(),
            queued_packets: Vec::with_capacity(32),
            sent_packets: Vec::with_capacity(32),
            acked_packets: Vec::with_capacity(32),
            received_packets: Vec::with_capacity(32),
            retrieved_packets: Vec::with_capacity(32),
            rtt_tracker: Duration::default(),
        };

        Self { info, state }
    }
}
