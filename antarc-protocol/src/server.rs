use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
};

use log::{debug, warn};

use crate::{
    events::*,
    packets::*,
    peers::{AwaitingConnectionAck, Connected, Peer, RequestingConnection, SendTo},
    scheduler, EventSystem, Protocol,
};

#[derive(Debug)]
pub struct Server {
    pub last_antarc_schedule_check: Duration,
    pub connection_id_tracker: ConnectionId,
    pub requesting_connection: HashMap<ConnectionId, Peer<RequestingConnection>>,
    pub awaiting_connection_ack: HashMap<ConnectionId, Peer<AwaitingConnectionAck>>,
    pub connected: HashMap<ConnectionId, Peer<Connected>>,
    pub ban_list: Vec<SocketAddr>,
}

impl Protocol<Server> {
    pub fn new_server() -> Self {
        let service = Server {
            last_antarc_schedule_check: Duration::default(),
            connection_id_tracker: ConnectionId::new(1).unwrap(),
            requesting_connection: HashMap::with_capacity(32),
            awaiting_connection_ack: HashMap::with_capacity(32),
            connected: HashMap::with_capacity(32),
            ban_list: Vec::with_capacity(32),
        };

        Self {
            packet_id_tracker: 0,
            timer: Instant::now(),
            service,
            events: EventSystem::new(),
            receiver_pipe: Vec::with_capacity(32),
        }
    }

    pub(crate) fn unexpected_connection(&self, address: &SocketAddr) -> Result<(), ProtocolError> {
        // NOTE(alex): A peer only changes RequestingConnection -> AwaitingConnectionAck
        // after a connection accepted/denied packet is sent.
        if Protocol::connection_request_another_state(
            &self.service.awaiting_connection_ack,
            &self.service.connected,
            address,
        ) {
            warn!("server: peer {:#?} already in another state.", address);
            Err(ProtocolError::PeerInAnotherState(address.clone()))
        } else if self.service.ban_list.contains(address) {
            warn!("server: peer {:#?} is in the ban list.", address);
            Err(ProtocolError::Banned(address.clone()))
        } else {
            Ok(())
        }
    }

    /// NOTE(alex): API function that feeds the internal* event pipe.
    pub fn on_received(&mut self, raw_packet: RawPacket<Server>) -> Result<(), ProtocolError> {
        let decoded = raw_packet.decode(self.timer.elapsed())?;
        match decoded {
            DecodedForServer::ConnectionRequest { packet } => {
                debug!("server: received connection request {:#?}.", packet);
                let _ = self.unexpected_connection(&packet.delivery.meta.address)?;

                let connection_id = if let Some((connection_id, peer)) = self
                    .service
                    .requesting_connection
                    .iter_mut()
                    .find(|(_, peer)| peer.address == packet.delivery.meta.address)
                {
                    peer.connection.attempts += 1;

                    connection_id.clone()
                } else {
                    let new_peer = Peer::new(
                        self.timer.elapsed(),
                        packet.delivery.meta.address,
                        packet.sequence.get(),
                    );

                    let connection_id = self.service.connection_id_tracker;
                    self.service
                        .requesting_connection
                        .insert(connection_id, new_peer);
                    self.service.connection_id_tracker =
                        ConnectionId::new(connection_id.get() + 1).unwrap();

                    connection_id
                };

                self.accept_connection(connection_id);
                Ok(())
            }
            DecodedForServer::DataTransfer { packet } => {
                debug!("server: received data transfer {:#?}.", packet);

                let connection_id = packet.message.connection_id;
                let payload = packet.message.payload;

                if let Some(peer) = self.service.connected.get_mut(&connection_id) {
                    debug!("server: peer is connected {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                } else if let Some(mut peer) =
                    self.service.awaiting_connection_ack.remove(&connection_id)
                {
                    debug!("server: peer is awaiting connection ack {:#?}.", peer);

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
            DecodedForServer::Fragment { .. } => todo!(),
            DecodedForServer::Heartbeat { packet } => {
                debug!("server: received heartbeat {:#?}.", packet);

                let connection_id = packet.message.connection_id;

                if let Some(peer) = self.service.connected.get_mut(&connection_id) {
                    debug!("server: peer is connected {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                } else if let Some(mut peer) =
                    self.service.awaiting_connection_ack.remove(&connection_id)
                {
                    debug!("server: peer is awaiting connection ack {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                    let connected = peer.connected(self.timer.elapsed(), connection_id);

                    self.service.connected.insert(connection_id, connected);
                }

                Ok(())
            }
        }
    }

    // TODO(alex) [mid] 2021-08-02: There must be a way to have a generic version of this function.
    // If `Messager` or some other `Packet` trait implements an `Into<SentEvent>` it would work.
    //
    // Check the `client.rs` file that contains a comment with this possible function.
    pub fn sent_connection_accepted(&mut self, packet: Packet<ToSend, ConnectionAccepted>) {
        let sent = packet.sent(self.timer.elapsed());

        if let Some(peer) = self
            .service
            .awaiting_connection_ack
            .get_mut(&sent.message.connection_id)
        {
            peer.sequence_tracker = peer.sequence_tracker.checked_add(1).unwrap();
        }

        self.events.reliable_sent.push(sent.into());
        // TODO(alex) [mid] 2021-08-02: There is one difference between this and the `Client`'s
        // version of "after send" handler. The server already put the `Peer` into
        // `AwaitingConnectionAck` when it received the initial connection request. So we don't
        // have to update anything here.
        //
        // The `Peer<AwaitingConnectionAck>` will change to `Peer<Connected>` when we receive an
        // ack for this reliable packet.
    }

    pub fn prepare_connection_accepted(
        &self,
        scheduled: Scheduled<Reliable, ConnectionAccepted>,
    ) -> Packet<ToSend, ConnectionAccepted> {
        let (sequence, ack) = self
            .service
            .awaiting_connection_ack
            .get(&scheduled.message.connection_id)
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .unwrap();

        let packet = scheduled.into_packet(sequence, ack, self.timer.elapsed());
        packet
    }

    /// NOTE(alex): API function for scheduling connection accepted packets, called by the user.
    ///
    /// - Moves `Peer<RequestingConnection>` -> `Peer<AwaitingConnectionAck>`.
    pub(crate) fn accept_connection(&mut self, connection_id: ConnectionId) {
        if let Some(peer) = self.service.requesting_connection.remove(&connection_id) {
            debug!("server: accept connection for id {:#?}.", connection_id);

            let message = ConnectionAccepted {
                meta: MetaMessage {
                    packet_type: CONNECTION_ACCEPTED,
                },
                connection_id,
            };
            let scheduled = Scheduled {
                packet_id: self.packet_id_tracker,
                address: peer.address,
                time: self.timer.elapsed(),
                reliability: Reliable {},
                message,
            };
            self.events.scheduler.push(scheduled.into());
            self.packet_id_tracker += 1;

            let awaiting_connection_ack = peer.await_connection_ack(self.timer.elapsed());
            self.service
                .awaiting_connection_ack
                .insert(connection_id, awaiting_connection_ack);
        } else {
            self.events
                .api
                .push(ProtocolError::NotFound(connection_id).into());
        }
    }

    /// NOTE(alex): Helper function to reduce duplication in `pub schedule`.
    ///
    /// TODO(alex) [low] 2021-08-08: Parts of this function could be simplified, or at least I think
    /// it's possible. Leaving it with low priority for now though.
    pub(crate) fn inner_schedule(
        &mut self,
        payload: Payload,
        reliability: ReliabilityType,
        connection_id: ConnectionId,
    ) -> Result<PacketId, ProtocolError> {
        if let Some(peer) = self.service.connected.get(&connection_id) {
            let packet_id = self.packet_id_tracker;

            let should_fragment = payload.len() > MAX_FRAGMENT_SIZE;
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
                            "server: schedule fragment {:?}/{:?}.",
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
                debug!("server: schedule non-fragment.");

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

            Ok(packet_id)
        } else {
            Err(ProtocolError::ScheduledNotConnected(connection_id))
        }
    }

    /// NOTE(alex): API function for scheduling data transfers only, called by the user.
    ///
    /// TODO(alex) [low] 2021-08-08: Most of the duplication problems were solved by moving into a
    /// separate helper function, a bit remains though.
    pub fn schedule(
        &mut self,
        reliability: ReliabilityType,
        send_to: SendTo,
        payload: Payload,
    ) -> Result<PacketId, ProtocolError> {
        let _ = self
            .service
            .connected
            .is_empty()
            .then(|| ())
            .ok_or(ProtocolError::NoPeersConnected)?;

        match send_to {
            SendTo::Single { connection_id } => {
                debug!("server: SendTo::Single scheduler {:#?}.", connection_id);

                let old_scheduler_length = self.events.scheduler.len();

                let packet_id = self.inner_schedule(payload, reliability, connection_id)?;

                if self.events.scheduler.len() > old_scheduler_length {
                    self.packet_id_tracker += 1;
                }

                Ok(packet_id)
            }
            SendTo::Multiple { connection_ids } => {
                debug!("server: SendTo::Multiple scheduler {:#?}.", connection_ids);

                let old_scheduler_length = self.events.scheduler.len();
                let packet_id = self.packet_id_tracker;

                for connection_id in connection_ids {
                    let _ = self.inner_schedule(payload.clone(), reliability, connection_id)?;
                }

                if self.events.scheduler.len() > old_scheduler_length {
                    self.packet_id_tracker += 1;
                }

                Ok(packet_id)
            }
            SendTo::Broadcast => {
                debug!("server: SendTo::Broadcast scheduler.");

                let old_scheduler_length = self.events.scheduler.len();
                let packet_id = self.packet_id_tracker;

                let connection_ids = self
                    .service
                    .connected
                    .values()
                    .map(|peer| peer.connection.connection_id)
                    .collect::<Vec<_>>();

                for connection_id in connection_ids {
                    let _ = self.inner_schedule(payload.clone(), reliability, connection_id)?;
                }

                if self.events.scheduler.len() > old_scheduler_length {
                    self.packet_id_tracker += 1;
                }

                Ok(packet_id)
            }
        }
    }

    pub(crate) fn connection_request_another_state(
        awaiting_connection_ack: &HashMap<ConnectionId, Peer<AwaitingConnectionAck>>,
        connected: &HashMap<ConnectionId, Peer<Connected>>,
        address: &SocketAddr,
    ) -> bool {
        awaiting_connection_ack
            .values()
            .any(|peer| peer.address == *address)
            || connected.values().any(|peer| peer.address == *address)
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
