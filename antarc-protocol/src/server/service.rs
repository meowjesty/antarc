use core::{ops::RangeBounds, time::Duration};
use std::{collections::HashMap, net::SocketAddr, vec::Drain};

use log::{debug, warn};

use crate::{errors::*, events::*, packets::*, peers::*, *};

#[derive(Debug)]
pub struct ServerScheduler {
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
pub struct ServerReliabilityHandler {
    list_sent_connection_accepted: Vec<Packet<Sent, ConnectionAccepted>>,
}

impl ServiceReliability for ServerReliabilityHandler {
    fn new(capacity: usize) -> Self {
        Self {
            list_sent_connection_accepted: Vec::with_capacity(capacity),
        }
    }
}

impl ServerReliabilityHandler {
    pub(crate) fn connection_accepted(&mut self, packet: Packet<Sent, ConnectionAccepted>) {
        self.list_sent_connection_accepted.push(packet);
    }

    fn resend_reliable_connection_accepted(
        &mut self,
        now: Duration,
    ) -> Option<Packet<ToSend, ConnectionAccepted>> {
        if let Some(packet) = self.list_sent_connection_accepted.pop() {
            if packet.delivery.meta.time + now > Duration::from_secs(1000) {
                let meta = MetaDelivery {
                    time: now,
                    address: packet.delivery.meta.address,
                };
                let delivery = ToSend {
                    id: packet.delivery.id,
                    meta,
                };
                let message = ConnectionAccepted {
                    meta: packet.message.meta,
                    connection_id: packet.message.connection_id,
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
pub struct Server {
    pub api: Vec<ProtocolEvent<ServerEvent>>,
    pub(crate) last_antarc_schedule_check: Duration,
    pub(crate) connection_id_tracker: ConnectionId,
    pub(crate) requesting_connection: HashMap<ConnectionId, Peer<RequestingConnection>>,
    pub(crate) awaiting_connection_ack: HashMap<ConnectionId, Peer<AwaitingConnectionAck>>,
    pub(crate) connected: HashMap<ConnectionId, Peer<Connected>>,
    pub(crate) ban_list: Vec<SocketAddr>,

    pub(crate) scheduler: Scheduler<ServerScheduler>,
    pub(crate) reliability_handler: ReliabilityHandler<ServerReliabilityHandler>,
}

impl Service for Server {
    type SchedulerType = ServerScheduler;
    type ReliabilityHandlerType = ServerReliabilityHandler;

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
}

impl Server {
    pub fn new() -> Self {
        Self {
            api: Vec::with_capacity(32),
            last_antarc_schedule_check: Duration::default(),
            connection_id_tracker: ConnectionId::new(1).unwrap(),
            requesting_connection: HashMap::with_capacity(32),
            awaiting_connection_ack: HashMap::with_capacity(32),
            connected: HashMap::with_capacity(32),
            ban_list: Vec::with_capacity(32),

            scheduler: Scheduler::new(32),
            reliability_handler: ReliabilityHandler::new(32),
        }
    }

    fn unexpected_connection(&self, address: &SocketAddr) -> Result<(), ProtocolError> {
        // NOTE(alex): A peer only changes RequestingConnection -> AwaitingConnectionAck
        // after a connection accepted/denied packet is sent.
        if self.connection_request_another_state(address) {
            warn!("server: peer {:#?} already in another state.", address);
            Err(ProtocolError::PeerInAnotherState(address.clone()))
        } else if self.ban_list.contains(address) {
            warn!("server: peer {:#?} is in the ban list.", address);
            Err(ProtocolError::Banned(address.clone()))
        } else {
            Ok(())
        }
    }

    /// NOTE(alex): API function for scheduling connection accepted packets, called by the user.
    ///
    /// - Moves `Peer<RequestingConnection>` -> `Peer<AwaitingConnectionAck>`.
    fn accept_connection(
        &mut self,
        connection_id: ConnectionId,
        packet_id: PacketId,
        time: Duration,
    ) {
        if let Some(peer) = self.requesting_connection.remove(&connection_id) {
            debug!("server: accept connection for id {:#?}.", connection_id);

            let message = ConnectionAccepted {
                meta: MetaMessage {
                    packet_type: CONNECTION_ACCEPTED,
                },
                connection_id,
            };
            let connection_accepted = Scheduled {
                packet_id,
                address: peer.address,
                time,
                reliability: Reliable {},
                message,
            };

            self.scheduler
                .service
                .connection_accepted(connection_accepted);

            let awaiting_connection_ack = peer.await_connection_ack(time);
            self.awaiting_connection_ack
                .insert(connection_id, awaiting_connection_ack);
        } else {
            self.api.push(ProtocolError::NotFound(connection_id).into());
        }
    }

    // TODO(alex) [mid] 2021-08-02: There must be a way to have a generic version of this function.
    // If `Messager` or some other `Packet` trait implements an `Into<SentEvent>` it would work.
    //
    // Check the `client.rs` file that contains a comment with this possible function.
    pub fn sent_connection_accepted(
        &mut self,
        packet: Packet<ToSend, ConnectionAccepted>,
        time: Duration,
    ) {
        let sent = packet.sent(time);

        if let Some(peer) = self
            .awaiting_connection_ack
            .get_mut(&sent.message.connection_id)
        {
            peer.sequence_tracker = peer.sequence_tracker.checked_add(1).unwrap();
        }

        self.reliability_handler.service.connection_accepted(sent);

        // TODO(alex) [mid] 2021-08-02: There is one difference between this and the `Client`'s
        // version of "after send" handler. The server already put the `Peer` into
        // `AwaitingConnectionAck` when it received the initial connection request. So we don't
        // have to update anything here.
        //
        // The `Peer<AwaitingConnectionAck>` will change to `Peer<Connected>` when we receive an
        // ack for this reliable packet.
    }

    /// NOTE(alex): API function that feeds the internal* event pipe.
    pub(crate) fn on_received(
        &mut self,
        raw_packet: RawPacket<Server>,
        packet_id: PacketId,
        time: Duration,
    ) -> Result<bool, ProtocolError> {
        let decoded = raw_packet.decode(time)?;
        match decoded {
            DecodedForServer::ConnectionRequest { packet } => {
                debug!("server: received connection request {:#?}.", packet);
                let _ = self.unexpected_connection(&packet.delivery.meta.address)?;

                let connection_id = if let Some((connection_id, peer)) = self
                    .requesting_connection
                    .iter_mut()
                    .find(|(_, peer)| peer.address == packet.delivery.meta.address)
                {
                    peer.connection.attempts += 1;

                    connection_id.clone()
                } else {
                    let new_peer =
                        Peer::new(time, packet.delivery.meta.address, packet.sequence.get());

                    let connection_id = self.connection_id_tracker;
                    self.requesting_connection.insert(connection_id, new_peer);
                    self.connection_id_tracker =
                        ConnectionId::new(connection_id.get() + 1).unwrap();

                    connection_id
                };

                self.accept_connection(connection_id, packet_id, time);
                Ok(true)
            }
            DecodedForServer::DataTransfer { packet } => {
                debug!("server: received data transfer {:#?}.", packet);

                let connection_id = packet.message.connection_id;
                let payload = packet.message.payload;

                if let Some(peer) = self.connected.get_mut(&connection_id) {
                    debug!("server: peer is connected {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                } else if let Some(mut peer) = self.awaiting_connection_ack.remove(&connection_id) {
                    debug!("server: peer was awaiting connection ack {:#?}.", peer);

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

                Ok(false)
            }
            DecodedForServer::Fragment { .. } => todo!(),
            DecodedForServer::Heartbeat { packet } => {
                debug!("server: received heartbeat {:#?}.", packet);

                let connection_id = packet.message.connection_id;

                if let Some(peer) = self.connected.get_mut(&connection_id) {
                    debug!("server: peer is connected {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                } else if let Some(mut peer) = self.awaiting_connection_ack.remove(&connection_id) {
                    debug!("server: peer is awaiting connection ack {:#?}.", peer);

                    peer.remote_ack_tracker = packet.sequence.get();
                    peer.local_ack_tracker = packet.ack;
                    let connected = peer.connected(time, connection_id);

                    self.connected.insert(connection_id, connected);
                }

                Ok(false)
            }
        }
    }

    pub(crate) fn create_connection_accepted(
        &self,
        scheduled: Scheduled<Reliable, ConnectionAccepted>,
        time: Duration,
    ) -> Packet<ToSend, ConnectionAccepted> {
        let (sequence, ack) = self
            .awaiting_connection_ack
            .get(&scheduled.message.connection_id)
            .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
            .expect("Creating a packet (connection accepted) should never fail!");

        let packet = scheduled.into_packet(sequence, ack, time);
        packet
    }

    pub(crate) fn sent_data_transfer(
        &mut self,
        packet: Packet<ToSend, DataTransfer>,
        time: Duration,
        reliability: ReliabilityType,
    ) {
        let sent = packet.sent(time);
        let connection_id = sent.message.connection_id;

        if let Some(connected) = self.connected.get_mut(&connection_id) {
            connected.sequence_tracker = connected.sequence_tracker.checked_add(1).unwrap();
        }

        if let ReliabilityType::Reliable = reliability {
            self.reliability_handler
                .list_sent_reliable_data_transfer
                .push(sent);
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
    pub(crate) fn schedule(
        &mut self,
        reliability: ReliabilityType,
        send_to: SendTo,
        payload: Payload,
        packet_id: PacketId,
        time: Duration,
    ) -> Result<PacketId, ProtocolError> {
        if self.connected.is_empty() {
            return Err(ProtocolError::NoPeersConnected);
        }

        let fragments = payload
            .chunks(MAX_FRAGMENT_SIZE)
            .enumerate()
            // TODO(alex) [mid] 2021-08-17: Change `Payload` to be `Arc<&[u8]>` so we don't need to
            // use `to_vec` here.
            //
            // ADD(alex) [low] 2021-08-18: This change has a big impact, and I don't think it's
            // possible to remove the allocation anyway, as `Arc::from(chunk)` would involve a
            // memcpy of `chunk`.
            // https://github.com/rust-lang/rust/pull/42565
            .map(|(index, chunk)| (index, Arc::new(chunk.to_vec())))
            .collect::<Vec<_>>();

        let fragment_total = fragments.len();
        let fragmented = fragment_total > 1;

        for (fragment_index, payload) in fragments.into_iter() {
            match send_to {
                SendTo::Single { connection_id } => {
                    debug!(
                        "server: SendTo::Single scheduling for {:#?}.",
                        connection_id
                    );

                    let address = self
                        .connected
                        .get(&connection_id)
                        .map(|peer| peer.address)
                        .ok_or(ProtocolError::ScheduledNotConnected(connection_id))?;

                    self.scheduler.schedule_for_connected_peer(
                        address,
                        payload.clone(),
                        reliability,
                        connection_id,
                        packet_id,
                        time,
                        fragmented,
                        fragment_index,
                        fragment_total,
                    );
                }
                SendTo::Broadcast => {
                    for (connection_id, address) in self
                        .connected
                        .iter()
                        .map(|(connection_id, peer)| (*connection_id, peer.address.clone()))
                    {
                        debug!(
                            "server: SendTo::Broadcast scheduling for {:#?}.",
                            connection_id
                        );
                        self.scheduler.schedule_for_connected_peer(
                            address,
                            payload.clone(),
                            reliability,
                            connection_id,
                            packet_id,
                            time,
                            fragmented,
                            fragment_index,
                            fragment_total,
                        );
                    }
                }
            }
        }

        Ok(packet_id + 1)
    }

    fn connection_request_another_state(&self, address: &SocketAddr) -> bool {
        self.awaiting_connection_ack
            .values()
            .any(|peer| peer.address == *address)
            || self.connected.values().any(|peer| peer.address == *address)
    }

    pub fn drain_connection_accepted<R: RangeBounds<usize>>(
        &mut self,
        range: R,
    ) -> Drain<Scheduled<Reliable, ConnectionAccepted>> {
        self.scheduler
            .service
            .list_scheduled_connection_accepted
            .drain(range)
    }

    pub(crate) fn resend_reliable_connection_accepted(
        &mut self,
        time: Duration,
    ) -> Option<Packet<ToSend, ConnectionAccepted>> {
        self.reliability_handler
            .service
            .resend_reliable_connection_accepted(time)
    }
}
