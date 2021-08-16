use core::ops::RangeBounds;
use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
    vec::Drain,
};

use log::{debug, warn};

use crate::{
    events::*,
    packets::*,
    peers::{AwaitingConnectionAck, Connected, Peer, RequestingConnection, SendTo},
    EventSystem, Protocol, Scheduler, Service, ServiceScheduler,
};

// TODO(alex) [high] 2021-08-10: It works, but there is a big overlap of scheduled packets here and
// on `ClientSchedule` (data transfers, and fragments), so this implies we need to abstract it one
// level higher, to some `Scheduler<Service>` (would be ideal).
#[derive(Debug)]
pub(crate) struct ServerScheduler {
    list_scheduled_connection_accepted: Vec<Scheduled<Reliable, ConnectionAccepted>>,
}

impl ServiceScheduler for ServerScheduler {
    fn new(capacity: usize) -> Self {
        Self {
            list_scheduled_connection_accepted: Vec::with_capacity(capacity),
        }
    }
}

impl ServerScheduler {
    pub(crate) fn connection_accepted(
        &mut self,
        scheduled: Scheduled<Reliable, ConnectionAccepted>,
    ) {
        self.list_scheduled_connection_accepted.push(scheduled);
    }
}

#[derive(Debug)]
pub struct Server {
    pub last_antarc_schedule_check: Duration,
    pub connection_id_tracker: ConnectionId,
    pub requesting_connection: HashMap<ConnectionId, Peer<RequestingConnection>>,
    pub awaiting_connection_ack: HashMap<ConnectionId, Peer<AwaitingConnectionAck>>,
    pub connected: HashMap<ConnectionId, Peer<Connected>>,
    pub ban_list: Vec<SocketAddr>,

    pub(crate) scheduler: Scheduler<ServerScheduler>,
    pub api: Vec<AntarcEvent<ServerEvent>>,
}

impl Service for Server {}

impl Server {
    pub fn drain_connection_accepted<R: RangeBounds<usize>>(
        &mut self,
        range: R,
    ) -> Drain<Scheduled<Reliable, ConnectionAccepted>> {
        self.scheduler
            .service
            .list_scheduled_connection_accepted
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

impl Protocol<Server> {
    pub fn new_server() -> Self {
        let service = Server {
            last_antarc_schedule_check: Duration::default(),
            connection_id_tracker: ConnectionId::new(1).unwrap(),
            requesting_connection: HashMap::with_capacity(32),
            awaiting_connection_ack: HashMap::with_capacity(32),
            connected: HashMap::with_capacity(32),
            ban_list: Vec::with_capacity(32),

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
                    debug!("server: peer was awaiting connection ack {:#?}.", peer);

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

    pub fn create_connection_accepted(
        &self,
        scheduled: Scheduled<Reliable, ConnectionAccepted>,
    ) -> Packet<ToSend, ConnectionAccepted> {
        let (sequence, ack) = self
            .service
            .awaiting_connection_ack
            .get(&scheduled.message.connection_id)
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .expect("Creating a packet (connection accepted) should never fail!");

        let packet = scheduled.into_packet(sequence, ack, self.timer.elapsed());
        packet
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
        let connection_id = sent.message.connection_id;

        if let Some(connected) = self.service.connected.get_mut(&connection_id) {
            connected.sequence_tracker = connected.sequence_tracker.checked_add(1).unwrap();
        }
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
            let connection_accepted = Scheduled {
                packet_id: self.packet_id_tracker,
                address: peer.address,
                time: self.timer.elapsed(),
                reliability: Reliable {},
                message,
            };

            self.service
                .scheduler
                .service
                .connection_accepted(connection_accepted);

            self.packet_id_tracker += 1;

            let awaiting_connection_ack = peer.await_connection_ack(self.timer.elapsed());
            self.service
                .awaiting_connection_ack
                .insert(connection_id, awaiting_connection_ack);
        } else {
            self.service
                .api
                .push(ProtocolError::NotFound(connection_id).into());
        }
    }

    /// NOTE(alex): Helper function to reduce duplication in `pub schedule`.
    ///
    /// TODO(alex) [low] 2021-08-08: Parts of this function could be simplified, or at least I think
    /// it's possible. Leaving it with low priority for now though.
    pub(crate) fn schedule_for_connected_peer(
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

                for (fragment_index, payload) in fragments.into_iter() {
                    debug!(
                        "server: schedule fragment {:?}/{:?}.",
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
                        peer.address.clone(),
                    );
                }
            } else {
                debug!("server: schedule non-fragment.");

                self.service.scheduler.schedule_data_transfer(
                    reliability,
                    packet_id,
                    connection_id,
                    payload.clone(),
                    self.timer.elapsed(),
                    peer.address,
                );
            }

            Ok(packet_id)
        } else {
            Err(ProtocolError::ScheduledNotConnected(connection_id))
        }
    }

    /// NOTE(alex): API function for scheduling data transfers only, called by the user.
    ///
    /// There are 2 choices:
    ///
    /// 1. schedule to single peer;
    /// 2. broadcast to every peer;
    ///
    /// If the user wants to send to multiple select peers, then they must call the single version
    /// multiple times.
    ///
    /// TODO(alex) [low] 2021-08-08: Most of the duplication problems were solved by moving into a
    /// separate helper function, a bit remains though.
    ///
    /// TODO(alex) [vlow] 2021-08-09: There was a `SendTo::Multiple` that took a list of connection
    /// ids, but this function can't properly handle failure state for this variant. To schedule for
    /// multiple peers like that, another return type must be had in case the user passes 1 or more
    /// not-connected connection ids. This could be some sort of `schedule_batch`.
    pub fn schedule(
        &mut self,
        reliability: ReliabilityType,
        send_to: SendTo,
        payload: Payload,
    ) -> Result<PacketId, ProtocolError> {
        if self.service.connected.is_empty() {
            return Err(ProtocolError::NoPeersConnected);
        }

        match send_to {
            SendTo::Single { connection_id } => {
                debug!("server: SendTo::Single scheduler {:#?}.", connection_id);

                let packet_id =
                    self.schedule_for_connected_peer(payload, reliability, connection_id)?;

                self.packet_id_tracker += 1;

                Ok(packet_id)
            }
            SendTo::Broadcast => {
                debug!("server: SendTo::Broadcast scheduler.");

                let packet_id = self.packet_id_tracker;

                let connection_ids = self
                    .service
                    .connected
                    .values()
                    .map(|peer| peer.connection.connection_id)
                    .collect::<Vec<_>>();

                for connection_id in connection_ids {
                    let _ = self
                        .schedule_for_connected_peer(payload.clone(), reliability, connection_id)
                        .expect("Scheduling a broadcast should be infallible!");
                }

                self.packet_id_tracker += 1;

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

    pub fn poll(&mut self) -> std::vec::Drain<AntarcEvent<ServerEvent>> {
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
