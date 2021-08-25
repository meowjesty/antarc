use core::{ops::RangeBounds, time::Duration};
use std::{collections::HashMap, net::SocketAddr, sync::Arc, vec::Drain};

use log::*;

use crate::{
    errors::*,
    events::*,
    packets::{decode::*, delivery::*, message::*, raw::*, scheduled::*, *},
    peers::*,
    scheduler::*,
    ReliabilityHandler, Service, ServiceReliability, ServiceScheduler,
};

#[derive(Debug)]
pub struct ClientScheduler {
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
pub struct ClientReliabilityHandler {
    list_sent_connection_request: Vec<Packet<Sent, ConnectionRequest>>,
}

impl ServiceReliability for ClientReliabilityHandler {
    fn new(capacity: usize) -> Self {
        Self {
            list_sent_connection_request: Vec::with_capacity(capacity),
        }
    }

    fn poll(&mut self, now: Duration) {
        if self
            .list_sent_connection_request
            .first()
            .map(|packet| (packet.delivery.meta.time + packet.delivery.ttl > now).then(|| ()))
            .is_some()
        {
            self.list_sent_connection_request.remove(0);
        }
    }
}

impl ClientReliabilityHandler {
    pub(crate) fn connection_request(&mut self, packet: Packet<Sent, ConnectionRequest>) {
        self.list_sent_connection_request.push(packet);
    }

    fn resend_reliable_connection_request(
        &mut self,
        now: Duration,
    ) -> Option<Packet<ToSend, ConnectionRequest>> {
        if let Some(packet) = self.list_sent_connection_request.pop() {
            if packet.delivery.meta.time + now > Duration::from_secs(1000) {
                let meta = MetaDelivery {
                    time: now,
                    address: packet.delivery.meta.address,
                };
                let delivery = ToSend {
                    id: packet.delivery.id,
                    meta,
                };
                let message = ConnectionRequest {
                    meta: packet.message.meta,
                };
                let result = Packet {
                    delivery,
                    sequence: packet.sequence,
                    ack: packet.ack,
                    message,
                };

                return Some(result);
            }
        }

        None
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

impl Service for Client {
    type SchedulerType = ClientScheduler;
    type ReliabilityHandlerType = ClientReliabilityHandler;
    type ConnectionMessage = ConnectionAccepted;

    fn scheduler(&self) -> &Scheduler<Self::SchedulerType> {
        &self.scheduler
    }

    fn scheduler_mut(&mut self) -> &mut Scheduler<Self::SchedulerType> {
        &mut self.scheduler
    }

    fn reliability_handler(&self) -> &ReliabilityHandler<Self::ReliabilityHandlerType> {
        &self.reliability_handler
    }

    fn reliability_handler_mut(&mut self) -> &mut ReliabilityHandler<Self::ReliabilityHandlerType> {
        &mut self.reliability_handler
    }

    fn connected(&self) -> &HashMap<ConnectionId, Peer<Connected>> {
        &self.connected
    }

    fn sent_data_transfer(
        &mut self,
        packet: Packet<ToSend, DataTransfer>,
        time: Duration,
        reliability: ReliabilityType,
        ttl: Duration,
    ) {
        let sent = packet.sent(time, ttl);
        let address = sent.delivery.meta.address;
        let connection_id = sent.message.connection_id;

        if let Some(peer) = self.awaiting_connection_ack.remove(&address) {
            self.connected
                .insert(connection_id, peer.connected(time, connection_id));
        }

        if let Some(connected) = self.connected.get_mut(&connection_id) {
            connected.sequence_tracker = connected.sequence_tracker.checked_add(1).unwrap();
        }

        if let ReliabilityType::Reliable = reliability {
            self.reliability_handler
                .list_sent_reliable_data_transfer
                .push(sent);
        }
    }

    fn sent_fragment(
        &mut self,
        packet: Packet<ToSend, Fragment>,
        time: Duration,
        reliability: ReliabilityType,
        ttl: Duration,
    ) {
        let sent = packet.sent(time, ttl);
        let address = sent.delivery.meta.address;
        let connection_id = sent.message.connection_id;

        if let Some(peer) = self.awaiting_connection_ack.remove(&address) {
            self.connected
                .insert(connection_id, peer.connected(time, connection_id));
        }

        // NOTE(alex): Only increase `Peer::sequence_tracker` for fragment if it's the last part.
        // The fragment's `sequence` is used as a `fragment_id`.
        if let Some(connected) = (sent.message.index == sent.message.total - 1)
            .then(|| ())
            .and(self.connected.get_mut(&connection_id))
        {
            connected.sequence_tracker = connected.sequence_tracker.checked_add(1).unwrap();
        }

        if let ReliabilityType::Reliable = reliability {
            self.reliability_handler
                .list_sent_reliable_fragment
                .push(sent);
        }
    }

    fn sent_heartbeat(
        &mut self,
        packet: Packet<ToSend, Heartbeat>,
        time: Duration,
        reliability: ReliabilityType,
        ttl: Duration,
    ) {
        let sent = packet.sent(time, ttl);
        let address = sent.delivery.meta.address;
        let connection_id = sent.message.connection_id;

        if let Some(peer) = self.awaiting_connection_ack.remove(&address) {
            self.connected
                .insert(connection_id, peer.connected(time, connection_id));
        }

        if let Some(connected) = self.connected.get_mut(&connection_id) {
            connected.sequence_tracker = connected.sequence_tracker.checked_add(1).unwrap();
        }

        if let ReliabilityType::Reliable = reliability {
            self.reliability_handler
                .list_sent_reliable_heartbeat
                .push(sent);
        }
    }

    const DEBUG_NAME: &'static str = "Client";
}

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
        ttl: Duration,
    ) {
        let sent = packet.sent(time, ttl);
        let address = sent.delivery.meta.address;

        let mut peer = self.requesting_connection.remove(&address).unwrap();
        peer.sequence_tracker = peer.sequence_tracker.checked_add(1).unwrap();

        self.reliability_handler.service.connection_request(sent);
        self.awaiting_connection_ack
            .insert(address, peer.await_connection_ack(time));
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
            DecodedForClient::Fragment { packet } => {
                debug!("client: received fragment {:#?}.", packet);

                let connection_id = packet.message.connection_id;

                if let Some(peer) = self.connected.get_mut(&connection_id) {
                    debug!("client: peer is connected {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                } else {
                    return Err(ProtocolError::NoPeersConnected);
                }

                let peer = self
                    .connected
                    .get_mut(&connection_id)
                    .expect("Peer must be connected!");
                let fragment_id = packet.sequence;
                let fragment_total = packet.message.total as usize;

                let last_fragment = match peer.connection.reassembler.get_mut(&fragment_id) {
                    Some(fragments) => {
                        fragments.push(packet);
                        fragments.len() == fragment_total
                    }
                    None => {
                        let mut fragments = Vec::with_capacity(fragment_total);
                        fragments.push(packet);
                        peer.connection.reassembler.insert(fragment_id, fragments);
                        false
                    }
                };

                if last_fragment {
                    debug!("client: received last fragment.");
                    let mut fragments = peer
                        .connection
                        .reassembler
                        .remove(&fragment_id)
                        .expect("Fragment must exist!");
                    debug!("client: fragments len {:#?}.", fragments.len());
                    fragments.sort_by(|a, b| a.sequence.cmp(&b.sequence));
                    debug!("client: fragments sorted {:#?}.", fragments);

                    let packet = Packet::from(fragments);
                    debug!("client: fragment became packet {:#?}.", packet);
                    self.api.push(ProtocolEvent::DataTransfer {
                        connection_id,
                        payload: Arc::try_unwrap(packet.message.payload).expect("Only owner!"),
                    })
                }

                Ok(())
            }
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
        self.requesting_connection.contains_key(remote_address)
            || self.awaiting_connection_ack.contains_key(remote_address)
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

    pub fn drain_connection_request<R: RangeBounds<usize>>(
        &mut self,
        range: R,
    ) -> Drain<Scheduled<Reliable, ConnectionRequest>> {
        self.scheduler
            .service
            .list_scheduled_connection_request
            .drain(range)
    }

    pub(crate) fn resend_reliable_connection_request(
        &mut self,
        time: Duration,
    ) -> Option<Packet<ToSend, ConnectionRequest>> {
        self.reliability_handler
            .service
            .resend_reliable_connection_request(time)
    }
}
