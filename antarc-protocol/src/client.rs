use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
};

use log::{debug, warn};

use crate::{
    events::{AntarcEvent, ProtocolError, ReceiverEvent, ScheduleEvent},
    packets::*,
    peers::{AwaitingConnectionAck, Connected, Peer, RequestingConnection},
    EventSystem, Protocol,
};

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

    // TODO(alex) [mid] 2021-08-02: There must be a way to have a generic version of this function.
    // If `Messager` or some other `Packet` trait implements an `Into<SentEvent>` it would work.
    pub fn sent_connection_request(&mut self, packet: Packet<ToSend, ConnectionRequest>) {
        let sent = packet.sent(self.timer.elapsed());
        let address = sent.delivery.meta.remote;

        let peer = self.service.requesting_connection.remove(&address).unwrap();

        self.events.reliable_sent.push(sent.into());
        self.service
            .awaiting_connection_ack
            .insert(address, peer.await_connection_ack(self.timer.elapsed()));
    }

    // pub fn sent<Message>(&mut self, packet: Packet<ToSend, Message>)
    // where
    //     Message: Messager + Encoder,
    // {
    //     let sent = packet.sent(self.timer.elapsed());
    //     self.events.reliable_sent.push(sent.into());
    // }

    pub fn prepare_connection_request(
        &self,
        scheduled: Scheduled<Reliable, ConnectionRequest>,
    ) -> Packet<ToSend, ConnectionRequest> {
        let (sequence, ack) = self
            .service
            .requesting_connection
            .values()
            .last()
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .unwrap();

        let packet = scheduled.into_packet(sequence, ack, self.timer.elapsed());
        packet
    }

    /// NOTE(alex): API function that feeds the internal* event pipe.
    pub fn received(&mut self, raw_packet: RawPacket) {
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
        raw_packet.decode(&mut self.events);
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
    /// TODO(alex) [mid] 2021-07-31: This function is duplication-city, most of the code inside the
    /// `if should_fragment else` is a copy of each other.
    pub fn schedule(&mut self, reliable: bool, payload: Payload) {
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
                debug!("server: schedule fragment.");

                let fragments = payload
                    .chunks(MAX_FRAGMENT_SIZE)
                    .enumerate()
                    .map(|(index, chunk)| (index, chunk.to_vec()))
                    .collect::<Vec<_>>();

                let fragment_total = fragments.len();

                let mut scheduling = fragments
                    .into_iter()
                    .map(|(fragment_index, payload)| {
                        let meta = MetaMessage {
                            packet_type: DATA_TRANSFER_FRAGMENTED,
                        };
                        let message = Fragment {
                            meta,
                            connection_id,
                            index: fragment_index as u8,
                            total: fragment_total as u8,
                            payload: payload.clone(),
                        };

                        let scheduling = if reliable {
                            let scheduled = Scheduled {
                                packet_id,
                                time: self.timer.elapsed(),
                                address: peer.address.clone(),
                                reliability: Reliable {},
                                message,
                            };

                            scheduled.into()
                        } else {
                            let scheduled = Scheduled {
                                packet_id,
                                time: self.timer.elapsed(),
                                address: peer.address.clone(),
                                reliability: Unreliable {},
                                message,
                            };

                            scheduled.into()
                        };

                        scheduling
                    })
                    .collect::<Vec<_>>();

                self.events.scheduler.append(&mut scheduling);
            } else {
                debug!("client: schedule non-fragment.");

                let meta = MetaMessage {
                    packet_type: DATA_TRANSFER_FRAGMENTED,
                };
                let message = DataTransfer {
                    meta,
                    connection_id,
                    payload: payload.clone(),
                };

                let scheduling = if reliable {
                    let scheduled = Scheduled {
                        packet_id,
                        time: self.timer.elapsed(),
                        address: peer.address.clone(),
                        reliability: Reliable {},
                        message,
                    };

                    scheduled.into()
                } else {
                    let scheduled = Scheduled {
                        packet_id,
                        time: self.timer.elapsed(),
                        address: peer.address.clone(),
                        reliability: Unreliable {},
                        message,
                    };

                    scheduled.into()
                };

                self.events.scheduler.push(scheduling);
            }
        }

        if self.events.scheduler.len() > old_scheduler_length {
            self.packet_id_tracker += 1;
        }
    }

    pub fn connection_accepted_another_state(
        requesting_connection: &HashMap<ConnectionId, Peer<RequestingConnection>>,
        connected: &HashMap<ConnectionId, Peer<Connected>>,
        packet: &Packet<Received, ConnectionAccepted>,
    ) -> bool {
        let address = packet.delivery.meta.remote;
        requesting_connection
            .values()
            .any(|peer| peer.address == address)
            || connected.values().any(|peer| peer.address == address)
    }

    pub fn poll(&mut self) -> std::vec::Drain<AntarcEvent> {
        for received in self.events.receiver.drain(..) {
            debug!("client: polling receiver events.");

            match received {
                ReceiverEvent::ConnectionRequest { packet } => {
                    warn!(
                        "client: received connection request, skipping {:#?}.",
                        packet
                    );
                    continue;
                }
                ReceiverEvent::ConnectionAccepted { packet } => {
                    debug!("client: received connection accepted {:#?}.", packet);
                    let address = packet.delivery.meta.remote;
                    let connection_id = packet.message.connection_id;

                    if self.service.requesting_connection.contains_key(&address)
                        || self.service.connected.contains_key(&connection_id)
                    {
                        warn!(
                            "client: peer already in another state, skipping {:#?}.",
                            packet
                        );
                        continue;
                    }

                    if let Some(peer) = self.service.awaiting_connection_ack.remove(&address) {
                        let connected = peer.connected(self.timer.elapsed(), connection_id);
                        self.service.connected.insert(connection_id, connected);
                    }
                }
                ReceiverEvent::DataTransfer { packet } => {
                    debug!("client: received data transfer {:#?}.", packet);

                    let address = packet.delivery.meta.remote;
                    let connection_id = packet.message.connection_id;
                    let payload = packet.message.payload;

                    if let Some(peer) = self.service.connected.get_mut(&connection_id) {
                        debug!("client: peer is connected {:#?}.", peer);

                        peer.remote_ack_tracker = packet.sequence.get();
                        peer.local_ack_tracker = packet.ack;
                    } else if let Some(mut peer) =
                        self.service.awaiting_connection_ack.remove(&address)
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
                }
                ReceiverEvent::Heartbeat { packet } => {
                    debug!("client: received heartbeat {:#?}.", packet);

                    let address = packet.delivery.meta.remote;
                    let connection_id = packet.message.connection_id;

                    if let Some(peer) = self.service.connected.get_mut(&connection_id) {
                        debug!("client: peer is connected {:#?}.", peer);

                        peer.remote_ack_tracker = packet.sequence.get();
                        peer.local_ack_tracker = packet.ack;
                    } else if let Some(mut peer) =
                        self.service.awaiting_connection_ack.remove(&address)
                    {
                        debug!("client: peer is awaiting connection ack {:#?}.", peer);

                        peer.remote_ack_tracker = packet.sequence.get();
                        peer.local_ack_tracker = packet.ack;
                        let connected = peer.connected(self.timer.elapsed(), connection_id);

                        self.service.connected.insert(connection_id, connected);
                    }
                }
            }
        }

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
