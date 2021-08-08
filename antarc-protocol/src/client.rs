use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
};

use log::{debug, warn};

use crate::{events::*, packets::*, peers::*, EventSystem, Protocol};

#[derive(Debug)]
pub struct Client {
    pub last_sent_time: Duration,
    pub requesting_connection: HashMap<SocketAddr, Peer<RequestingConnection>>,
    pub awaiting_connection_ack: HashMap<SocketAddr, Peer<AwaitingConnectionAck>>,
    pub connected: HashMap<ConnectionId, Peer<Connected>>,
}

impl Protocol<Client> {
    pub fn new_client() -> Self {
        let service = Client {
            last_sent_time: Duration::default(),
            requesting_connection: HashMap::with_capacity(32),
            awaiting_connection_ack: HashMap::with_capacity(32),
            connected: HashMap::with_capacity(32),
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
            .unwrap();

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
            .unwrap();

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

                    self.events.api.push(AntarcEvent::DataTransfer {
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

        self.events.scheduler.push(connection_request.into());
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
            self.events
                .api
                .push(AntarcEvent::Fail(ProtocolError::NoPeersConnected));
        }

        let should_fragment = payload.len() > MAX_FRAGMENT_SIZE;

        let old_scheduler_length = self.events.scheduler.len();

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

                let mut schedule_events = fragments
                    .into_iter()
                    .map(|(fragment_index, payload)| {
                        debug!(
                            "client: schedule fragment {:?}/{:?}.",
                            fragment_index, fragment_total
                        );
                        // TODO(alex) [mid] 2021-08-08: This and the data transfer part are
                        // finnicky, if I call the wrong `new` function, we could end up with both
                        // sides creating the same `ScheduleEvent`.
                        //
                        // Maybe one simple way of solving this would be to put the `Reliable` and
                        // `Unreliable` types inside the `ReliabilityType::Reliable(Reliable)` for
                        // example, and pass it into the `new` functions.
                        let schedule_event = match reliability {
                            ReliabilityType::Reliable => {
                                let scheduled = Scheduled::new_reliable_fragment(
                                    packet_id,
                                    connection_id,
                                    payload.clone(),
                                    fragment_index,
                                    fragment_total,
                                    self.timer.elapsed(),
                                    peer.address.clone(),
                                );

                                scheduled.into()
                            }
                            ReliabilityType::Unreliable => {
                                let scheduled = Scheduled::new_unreliable_fragment(
                                    packet_id,
                                    connection_id,
                                    payload.clone(),
                                    fragment_index,
                                    fragment_total,
                                    self.timer.elapsed(),
                                    peer.address.clone(),
                                );

                                scheduled.into()
                            }
                        };

                        schedule_event
                    })
                    .collect::<Vec<_>>();

                self.events.scheduler.append(&mut schedule_events);
            } else {
                debug!("client: schedule non-fragment.");

                // TODO(alex) [mid] 2021-08-08: This and the fragment part are finnicky, if I call
                // the wrong `new` function, we could end up with both sides creating the same
                // `ScheduleEvent`.
                let schedule_event = match reliability {
                    ReliabilityType::Reliable => {
                        let scheduled = Scheduled::new_reliable_data_transfer(
                            packet_id,
                            connection_id,
                            payload.clone(),
                            self.timer.elapsed(),
                            peer.address,
                        );

                        scheduled.into()
                    }
                    ReliabilityType::Unreliable => {
                        let scheduled = Scheduled::new_unreliable_data_transfer(
                            packet_id,
                            connection_id,
                            payload.clone(),
                            self.timer.elapsed(),
                            peer.address,
                        );

                        scheduled.into()
                    }
                };

                self.events.scheduler.push(schedule_event);
            }

            if self.events.scheduler.len() > old_scheduler_length {
                self.packet_id_tracker += 1;
            }

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

    pub fn poll(&mut self) -> std::vec::Drain<AntarcEvent> {
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

        self.events.api.drain(..)
    }
}
