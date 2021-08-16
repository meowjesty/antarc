use core::ops::RangeBounds;
use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
    vec::Drain,
};

use log::{debug, warn};

use crate::{
    events::*, packets::*, peers::*, EventSystem, Protocol, Scheduler, Service, ServiceScheduler,
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
pub struct Client {
    pub last_sent_time: Duration,
    pub requesting_connection: HashMap<SocketAddr, Peer<RequestingConnection>>,
    pub awaiting_connection_ack: HashMap<SocketAddr, Peer<AwaitingConnectionAck>>,
    pub connected: HashMap<ConnectionId, Peer<Connected>>,

    pub(crate) scheduler: Scheduler<ClientScheduler>,
    pub api: Vec<AntarcEvent<ClientEvent>>,
}

impl Service for Client {}

impl Client {
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

impl Protocol<Client> {
    pub fn new_client() -> Self {
        let service = Client {
            last_sent_time: Duration::default(),
            requesting_connection: HashMap::with_capacity(32),
            awaiting_connection_ack: HashMap::with_capacity(32),
            connected: HashMap::with_capacity(32),

            scheduler: Scheduler::new(32),
            api: Vec::with_capacity(32),
        };

        Self {
            packet_id_tracker: 0,
            timer: Instant::now(),
            service,
            events: EventSystem::new(),
            receiver_pipe: Vec::with_capacity(32),
        }
    }

    // REGION(alex): Connection Request
    pub fn create_connection_request(
        &self,
        scheduled: Scheduled<Reliable, ConnectionRequest>,
    ) -> Packet<ToSend, ConnectionRequest> {
        let (sequence, ack) = self
            .service
            .requesting_connection
            .get(&scheduled.address)
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .expect("Creating a packet (connection request) should never fail!");

        let packet = scheduled.into_packet(sequence, ack, self.timer.elapsed());
        packet
    }

    // TODO(alex) [mid] 2021-08-02: There must be a way to have a generic version of this function.
    // If `Messager` or some other `Packet` trait implements an `Into<SentEvent>` it would work.
    pub fn sent_connection_request(&mut self, packet: Packet<ToSend, ConnectionRequest>) {
        let sent = packet.sent(self.timer.elapsed());
        let address = sent.delivery.meta.address;

        let mut peer = self.service.requesting_connection.remove(&address).unwrap();
        peer.sequence_tracker = peer.sequence_tracker.checked_add(1).unwrap();

        self.events.reliable_sent.push(sent.into());
        self.service
            .awaiting_connection_ack
            .insert(address, peer.await_connection_ack(self.timer.elapsed()));
    }

    // REGION(alex): Data Transfer
    pub fn create_unreliable_data_transfer(
        &self,
        scheduled: Scheduled<Unreliable, DataTransfer>,
    ) -> Packet<ToSend, DataTransfer> {
        let (sequence, ack) = self
            .service
            .connected
            .get(&scheduled.message.connection_id)
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .expect("Creating a packet (unreliable data transfer) should never fail!");

        let packet = scheduled.into_packet(sequence, ack, self.timer.elapsed());
        packet
    }

    pub fn sent_data_transfer(&mut self, packet: Packet<ToSend, DataTransfer>) {
        let sent = packet.sent(self.timer.elapsed());
        let address = sent.delivery.meta.address;
        let connection_id = sent.message.connection_id;

        if let Some(peer) = self.service.awaiting_connection_ack.remove(&address) {
            self.service.connected.insert(
                connection_id,
                peer.connected(self.timer.elapsed(), connection_id),
            );
        }

        if let Some(connected) = self.service.connected.get_mut(&connection_id) {
            connected.sequence_tracker = connected.sequence_tracker.checked_add(1).unwrap();
        }
    }

    /// NOTE(alex): API function that feeds the internal* event pipe.
    pub fn on_received(&mut self, raw_packet: RawPacket<Client>) -> Result<(), ProtocolError> {
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
        let decoded = raw_packet.decode(self.timer.elapsed())?;
        match decoded {
            DecodedForClient::ConnectionAccepted { packet } => {
                debug!("client: received connection accepted {:#?}.", packet);
                let address = packet.delivery.meta.address;
                let connection_id = packet.message.connection_id;

                if self.service.requesting_connection.contains_key(&address)
                    || self.service.connected.contains_key(&connection_id)
                {
                    warn!(
                        "client: peer already in another state, skipping {:#?}.",
                        packet
                    );
                    return Err(ProtocolError::PeerInAnotherState(
                        packet.delivery.meta.address,
                    ));
                }

                if let Some(peer) = self.service.awaiting_connection_ack.remove(&address) {
                    let mut connected = peer.connected(self.timer.elapsed(), connection_id);

                    connected.local_ack_tracker = packet.ack;
                    connected.remote_ack_tracker = packet.sequence.get();

                    self.service.connected.insert(connection_id, connected);
                }

                Ok(())
            }
            DecodedForClient::DataTransfer { packet } => {
                debug!("client: received data transfer {:#?}.", packet);

                let address = packet.delivery.meta.address;
                let connection_id = packet.message.connection_id;
                let payload = packet.message.payload;

                if let Some(peer) = self.service.connected.get_mut(&connection_id) {
                    debug!("client: peer is connected {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                } else if let Some(mut peer) = self.service.awaiting_connection_ack.remove(&address)
                {
                    debug!("client: peer is awaiting connection ack {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                    let connected = peer.connected(self.timer.elapsed(), connection_id);

                    self.service.connected.insert(connection_id, connected);

                    self.service.api.push(AntarcEvent::DataTransfer {
                        connection_id,
                        payload,
                    });
                }

                Ok(())
            }
            DecodedForClient::Fragment { .. } => todo!(),
            DecodedForClient::Heartbeat { packet } => {
                debug!("client: received heartbeat {:#?}.", packet);

                let address = packet.delivery.meta.address;
                let connection_id = packet.message.connection_id;

                if let Some(peer) = self.service.connected.get_mut(&connection_id) {
                    debug!("client: peer is connected {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                } else if let Some(mut peer) = self.service.awaiting_connection_ack.remove(&address)
                {
                    debug!("client: peer is awaiting connection ack {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                    let connected = peer.connected(self.timer.elapsed(), connection_id);

                    self.service.connected.insert(connection_id, connected);
                }

                Ok(())
            }
        }
    }

    pub fn known_peer(&self, remote_address: &SocketAddr) -> bool {
        self.service
            .requesting_connection
            .contains_key(&remote_address)
            || self
                .service
                .awaiting_connection_ack
                .contains_key(&remote_address)
            || self
                .service
                .connected
                .values()
                .any(|peer| peer.address == *remote_address)
    }

    pub fn connect(&mut self, remote_address: SocketAddr) -> Result<(), ProtocolError> {
        if self.known_peer(&remote_address) {
            warn!(
                "client: peer already in another state, skipping connect {:#?}.",
                remote_address
            );
            return Err(ProtocolError::AlreadyConnectingToPeer(remote_address));
        }

        let requesting_connection = Peer::new(self.timer.elapsed(), remote_address, 0);
        let packet_id = self.packet_id_tracker;
        let connection_request =
            Scheduled::connection_request(packet_id, remote_address, self.timer.elapsed());

        self.service
            .scheduler
            .service
            .connection_request(connection_request);

        self.packet_id_tracker += 1;

        self.service
            .requesting_connection
            .insert(remote_address, requesting_connection);
        Ok(())
    }

    /// NOTE(alex): API function for scheduling data transfers only, called by the user.
    pub fn schedule(
        &mut self,
        reliability: ReliabilityType,
        payload: Payload,
    ) -> Result<PacketId, ProtocolError> {
        if self.service.connected.is_empty() {
            self.service
                .api
                .push(AntarcEvent::Fail(ProtocolError::NoPeersConnected));
        }

        let should_fragment = payload.len() > MAX_FRAGMENT_SIZE;

        if let Some(peer) = self.service.connected.values().last() {
            let packet_id = self.packet_id_tracker;
            let connection_id = peer.connection.connection_id;

            if should_fragment {
                let fragments = payload
                    .chunks(MAX_FRAGMENT_SIZE)
                    .enumerate()
                    .map(|(index, chunk)| (index, chunk.to_vec()))
                    .collect::<Vec<_>>();

                let fragment_total = fragments.len();

                for (fragment_index, payload) in fragments.into_iter() {
                    debug!(
                        "client: schedule fragment {:?}/{:?}.",
                        fragment_index, fragment_total
                    );

                    self.service.scheduler.schedule_fragment(
                        reliability,
                        packet_id,
                        connection_id,
                        payload.clone(),
                        fragment_index,
                        fragment_total,
                        self.timer.elapsed(),
                        peer.address,
                    );
                }
            } else {
                debug!("client: schedule non-fragment.");

                self.service.scheduler.schedule_data_transfer(
                    reliability,
                    packet_id,
                    connection_id,
                    payload.clone(),
                    self.timer.elapsed(),
                    peer.address,
                );
            }

            self.packet_id_tracker += 1;

            Ok(packet_id)
        } else {
            Err(ProtocolError::NoPeersConnected)
        }
    }

    pub fn connection_accepted_another_state(
        requesting_connection: &HashMap<ConnectionId, Peer<RequestingConnection>>,
        connected: &HashMap<ConnectionId, Peer<Connected>>,
        packet: &Packet<Received, ConnectionAccepted>,
    ) -> bool {
        let address = packet.delivery.meta.address;
        requesting_connection
            .values()
            .any(|peer| peer.address == address)
            || connected.values().any(|peer| peer.address == address)
    }

    pub fn poll(&mut self) -> std::vec::Drain<AntarcEvent<ClientEvent>> {
        // TODO(alex) [mid] 2021-07-30: Handle `scheduler` pipe of events. But how exactly?
        // The network will check if socket is ready, then call a `make_packet` that will take some
        // scheduled from the scheduler pipe, but how does it handle reliability?
        //
        // ADD(alex) [mid] 2021-08-01: After sending a reliable packet, it should be put in a list
        // of `SentReliable` or whatever packets, and these packets are checked via their
        // `meta.time` and resent after some time has passed, and they've not been acked yet.
        //
        // No need to convert such a packet into a `Scheduled`, the easier approach would be to have
        // a secondary `send_non_acked` function, that runs before the common `send`, and it takes
        // a packet from this reliable list and re-sends it, with updated time.

        self.service.api.drain(..)
    }
}
