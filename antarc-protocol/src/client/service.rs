use core::{ops::RangeBounds, time::Duration};
use std::{collections::HashMap, net::SocketAddr, sync::Arc, vec::Drain};

use log::*;

use crate::{
    errors::*, events::*, packets::*, peers::*, ReliabilityHandler, Scheduler, Service,
    ServiceReliability, ServiceScheduler,
};

#[derive(Debug)]
pub(crate) struct ClientScheduler {
    list_scheduled_connection_request: Vec<Scheduled<Reliable, ConnectionRequest>>,
}

impl ClientScheduler {
    pub(crate) fn connection_request(&mut self, scheduled: Scheduled<Reliable, ConnectionRequest>) {
        self.list_scheduled_connection_request.push(scheduled);
    }
}

impl ServiceScheduler for ClientScheduler {
    fn new(capacity: usize) -> Self {
        Self {
            list_scheduled_connection_request: Vec::with_capacity(capacity),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ClientReliabilityHandler {
    list_sent_connection_request: Vec<Packet<Sent, ConnectionRequest>>,
}

impl ServiceReliability for ClientReliabilityHandler {
    fn new(capacity: usize) -> Self {
        Self {
            list_sent_connection_request: Vec::with_capacity(capacity),
        }
    }
}

impl ClientReliabilityHandler {
    pub(crate) fn connection_request(&mut self, packet: Packet<Sent, ConnectionRequest>) {
        self.list_sent_connection_request.push(packet);
    }
}

#[derive(Debug)]
pub struct Client {
    pub api: Vec<ProtocolEvent<ClientEvent>>,
    pub(crate) last_sent_time: Duration,
    pub(crate) requesting_connection: HashMap<SocketAddr, Peer<RequestingConnection>>,
    pub(crate) awaiting_connection_ack: HashMap<SocketAddr, Peer<AwaitingConnectionAck>>,
    pub(crate) connected: HashMap<ConnectionId, Peer<Connected>>,

    pub(crate) scheduler: Scheduler<ClientScheduler>,
    pub(crate) reliability_handler: ReliabilityHandler<ClientReliabilityHandler>,
}

impl Service for Client {}

impl Client {
    pub(crate) fn new() -> Self {
        let service = Client {
            api: Vec::with_capacity(32),

            last_sent_time: Duration::default(),
            requesting_connection: HashMap::with_capacity(32),
            awaiting_connection_ack: HashMap::with_capacity(32),
            connected: HashMap::with_capacity(32),

            scheduler: Scheduler::new(32),
            reliability_handler: ReliabilityHandler::new(32),
        };

        service
    }

    // REGION(alex): Connection Request
    pub(crate) fn create_connection_request(
        &self,
        scheduled: Scheduled<Reliable, ConnectionRequest>,
        time: Duration,
    ) -> Packet<ToSend, ConnectionRequest> {
        let (sequence, ack) = self
            .requesting_connection
            .get(&scheduled.address)
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .expect("Creating a packet (connection request) should never fail!");

        let packet = scheduled.into_packet(sequence, ack, time);
        packet
    }

    /// TODO(alex) [mid] 2021-08-02: There must be a way to have a generic version of this function.
    /// If `Messager` or some other `Packet` trait implements an `Into<SentEvent>` it would work.
    pub(crate) fn sent_connection_request(
        &mut self,
        packet: Packet<ToSend, ConnectionRequest>,
        time: Duration,
    ) {
        let sent = packet.sent(time);
        let address = sent.delivery.meta.address;

        let mut peer = self.requesting_connection.remove(&address).unwrap();
        peer.sequence_tracker = peer.sequence_tracker.checked_add(1).unwrap();

        self.reliability_handler.service.connection_request(sent);
        self.awaiting_connection_ack
            .insert(address, peer.await_connection_ack(time));
    }

    // REGION(alex): Data Transfer
    pub(crate) fn create_unreliable_data_transfer(
        &self,
        scheduled: Scheduled<Unreliable, DataTransfer>,
        time: Duration,
    ) -> Packet<ToSend, DataTransfer> {
        let (sequence, ack) = self
            .connected
            .get(&scheduled.message.connection_id)
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .expect("Creating a packet (unreliable data transfer) should never fail!");

        let packet = scheduled.into_packet(sequence, ack, time);
        packet
    }

    pub(crate) fn sent_data_transfer(
        &mut self,
        packet: Packet<ToSend, DataTransfer>,
        time: Duration,
    ) {
        let sent = packet.sent(time);
        let address = sent.delivery.meta.address;
        let connection_id = sent.message.connection_id;

        if let Some(peer) = self.awaiting_connection_ack.remove(&address) {
            self.connected
                .insert(connection_id, peer.connected(time, connection_id));
        }

        if let Some(connected) = self.connected.get_mut(&connection_id) {
            connected.sequence_tracker = connected.sequence_tracker.checked_add(1).unwrap();
        }
    }

    /// NOTE(alex): API function that feeds the internal* event pipe.
    pub(crate) fn on_received(
        &mut self,
        raw_packet: RawPacket<Client>,
        time: Duration,
    ) -> Result<(), ProtocolError> {
        // TODO(alex) [low] 2021-08-01: There will be a conflict when switching up to
        // `ClientEvent` and `ServerEvent` separation. This issue seems unavoidable, as a Client
        // should error on some types of packet, while the `Server` errors on others? Nope, the
        // decoding doesn't care about the service type!
        //
        // To avoid the issue, there must be an event type that is common to both `Client` and
        // `Server`, something like `DecodedEvent`, and from such an event, a server may extract
        // only appropriate packet types (whenever this is handled), and so does the client.
        //
        // The `DecodedEvent` accepts any type of packet.
        //
        // ADD(alex) [mid] 2021-08-06: I've tried tackling this with the generic types approach, and
        // now we have 2 distinct event types, one for client (here), and one for server.
        //
        // The big issue now is that I can't see a way to avoid a bunch of the code duplication we
        // end up having both in the different `decode` functions, and here when we handle their
        // results.
        //
        // Plenty of packet types are compatible with both `Service`s, but how do I make it work?
        let decoded = raw_packet.decode(time)?;
        match decoded {
            DecodedForClient::ConnectionAccepted { packet } => {
                debug!("client: received connection accepted {:#?}.", packet);
                let address = packet.delivery.meta.address;
                let connection_id = packet.message.connection_id;

                if self.requesting_connection.contains_key(&address)
                    || self.connected.contains_key(&connection_id)
                {
                    warn!(
                        "client: peer already in another state, skipping {:#?}.",
                        packet
                    );
                    return Err(ProtocolError::PeerInAnotherState(
                        packet.delivery.meta.address,
                    ));
                }

                if let Some(peer) = self.awaiting_connection_ack.remove(&address) {
                    let mut connected = peer.connected(time, connection_id);

                    connected.local_ack_tracker = packet.ack;
                    connected.remote_ack_tracker = packet.sequence.get();

                    self.connected.insert(connection_id, connected);
                }

                Ok(())
            }
            DecodedForClient::DataTransfer { packet } => {
                debug!("client: received data transfer {:#?}.", packet);

                let address = packet.delivery.meta.address;
                let connection_id = packet.message.connection_id;
                let payload = packet.message.payload;

                if let Some(peer) = self.connected.get_mut(&connection_id) {
                    debug!("client: peer is connected {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                } else if let Some(mut peer) = self.awaiting_connection_ack.remove(&address) {
                    debug!("client: peer is awaiting connection ack {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                    let connected = peer.connected(time, connection_id);

                    self.connected.insert(connection_id, connected);

                    self.api.push(ProtocolEvent::DataTransfer {
                        connection_id,

                        // TODO(alex) [low] 2021-08-16: Right now this is completely safe, as the
                        // decoded packet is the only owner of this `Arc<Payload>`. Only the sending
                        // side has to deal with shared ownership.
                        //
                        // This means that `Arc<Payload>` here doesn't actually make any sense, it
                        // should be the only owner.
                        payload: Arc::try_unwrap(payload).unwrap(),
                    });
                }

                Ok(())
            }
            DecodedForClient::Fragment { .. } => todo!(),
            DecodedForClient::Heartbeat { packet } => {
                debug!("client: received heartbeat {:#?}.", packet);

                let address = packet.delivery.meta.address;
                let connection_id = packet.message.connection_id;

                if let Some(peer) = self.connected.get_mut(&connection_id) {
                    debug!("client: peer is connected {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                } else if let Some(mut peer) = self.awaiting_connection_ack.remove(&address) {
                    debug!("client: peer is awaiting connection ack {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                    let connected = peer.connected(time, connection_id);

                    self.connected.insert(connection_id, connected);
                }

                Ok(())
            }
        }
    }

    fn known_peer(&self, remote_address: &SocketAddr) -> bool {
        self.requesting_connection.contains_key(&remote_address)
            || self.awaiting_connection_ack.contains_key(&remote_address)
            || self
                .connected
                .values()
                .any(|peer| peer.address == *remote_address)
    }

    pub(crate) fn connect(
        &mut self,
        remote_address: SocketAddr,
        packet_id: PacketId,
        time: Duration,
    ) -> Result<(), ProtocolError> {
        if self.known_peer(&remote_address) {
            warn!(
                "client: peer already in another state, skipping connect {:#?}.",
                remote_address
            );
            return Err(ProtocolError::AlreadyConnectingToPeer(remote_address));
        }

        let requesting_connection = Peer::new(time, remote_address, 0);
        let connection_request = Scheduled::connection_request(packet_id, remote_address, time);

        self.scheduler
            .service
            .connection_request(connection_request);

        self.requesting_connection
            .insert(remote_address, requesting_connection);

        Ok(())
    }

    /// NOTE(alex): API function for scheduling data transfers only, called by the user.
    pub(crate) fn schedule(
        &mut self,
        reliability: ReliabilityType,
        payload: Arc<Payload>,
        packet_id: PacketId,
        time: Duration,
    ) -> Result<PacketId, ProtocolError> {
        if self.connected.is_empty() {
            self.api
                .push(ProtocolEvent::Fail(ProtocolError::NoPeersConnected));
        }

        let should_fragment = payload.len() > MAX_FRAGMENT_SIZE;

        if let Some(peer) = self.connected.values().last() {
            let connection_id = peer.connection.connection_id;

            if should_fragment {
                let fragments = payload
                    .chunks(MAX_FRAGMENT_SIZE)
                    .enumerate()
                    .map(|(index, chunk)| (index, Arc::new(chunk.to_vec())))
                    .collect::<Vec<_>>();

                let fragment_total = fragments.len();

                for (fragment_index, payload) in fragments.into_iter() {
                    debug!(
                        "client: schedule fragment {:?}/{:?}.",
                        fragment_index, fragment_total
                    );

                    self.scheduler.schedule_fragment(
                        reliability,
                        packet_id,
                        connection_id,
                        payload,
                        fragment_index,
                        fragment_total,
                        time,
                        peer.address,
                    );
                }
            } else {
                debug!("client: schedule non-fragment.");

                self.scheduler.schedule_data_transfer(
                    reliability,
                    packet_id,
                    connection_id,
                    payload,
                    time,
                    peer.address,
                );
            }

            Ok(packet_id)
        } else {
            Err(ProtocolError::NoPeersConnected)
        }
    }

    pub fn drain_connection_request<R: RangeBounds<usize>>(
        &mut self,
        range: R,
    ) -> Drain<Scheduled<Reliable, ConnectionRequest>> {
        self.scheduler
            .service
            .list_scheduled_connection_request
            .drain(range)
    }

    pub fn drain_unreliable_data_transfer<R: RangeBounds<usize>>(
        &mut self,
        range: R,
    ) -> Drain<Scheduled<Unreliable, DataTransfer>> {
        self.scheduler
            .list_scheduled_unreliable_data_transfer
            .drain(range)
    }
}
