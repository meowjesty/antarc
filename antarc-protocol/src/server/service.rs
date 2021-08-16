use core::{ops::RangeBounds, time::Duration};
use std::{collections::HashMap, net::SocketAddr, vec::Drain};

use log::{debug, warn};

use crate::{
    events::*,
    packets::*,
    peers::{AwaitingConnectionAck, Connected, Peer, RequestingConnection, SendTo},
    Scheduler, Service, ServiceScheduler,
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
    pub fn new() -> Self {
        Self {
            last_antarc_schedule_check: Duration::default(),
            connection_id_tracker: ConnectionId::new(1).unwrap(),
            requesting_connection: HashMap::with_capacity(32),
            awaiting_connection_ack: HashMap::with_capacity(32),
            connected: HashMap::with_capacity(32),
            ban_list: Vec::with_capacity(32),

            scheduler: Scheduler::new(32),
            api: Vec::with_capacity(32),
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

                    self.api.push(AntarcEvent::DataTransfer {
                        connection_id,
                        payload,
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
        let connection_id = sent.message.connection_id;

        if let Some(connected) = self.connected.get_mut(&connection_id) {
            connected.sequence_tracker = connected.sequence_tracker.checked_add(1).unwrap();
        }
    }

    /// NOTE(alex): Helper function to reduce duplication in `pub schedule`.
    ///
    /// TODO(alex) [low] 2021-08-08: Parts of this function could be simplified, or at least I think
    /// it's possible. Leaving it with low priority for now though.
    fn schedule_for_connected_peer(
        &mut self,
        payload: Payload,
        reliability: ReliabilityType,
        connection_id: ConnectionId,
        packet_id: PacketId,
        time: Duration,
    ) -> Result<PacketId, ProtocolError> {
        if let Some(peer) = self.connected.get(&connection_id) {
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

                    self.scheduler.schedule_fragment(
                        reliability,
                        packet_id,
                        connection_id,
                        payload.clone(),
                        fragment_index,
                        fragment_total,
                        time,
                        peer.address.clone(),
                    );
                }
            } else {
                debug!("server: schedule non-fragment.");

                self.scheduler.schedule_data_transfer(
                    reliability,
                    packet_id,
                    connection_id,
                    payload.clone(),
                    time,
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

        match send_to {
            SendTo::Single { connection_id } => {
                debug!("server: SendTo::Single scheduler {:#?}.", connection_id);

                let packet_id = self.schedule_for_connected_peer(
                    payload,
                    reliability,
                    connection_id,
                    packet_id,
                    time,
                )?;

                Ok(packet_id)
            }
            SendTo::Broadcast => {
                debug!("server: SendTo::Broadcast scheduler.");

                let connection_ids = self
                    .connected
                    .values()
                    .map(|peer| peer.connection.connection_id)
                    .collect::<Vec<_>>();

                for connection_id in connection_ids {
                    let _ = self
                        .schedule_for_connected_peer(
                            payload.clone(),
                            reliability,
                            connection_id,
                            packet_id,
                            time,
                        )
                        .expect("Scheduling a broadcast should be infallible!");
                }

                Ok(packet_id)
            }
        }
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

    pub fn drain_unreliable_data_transfer<R: RangeBounds<usize>>(
        &mut self,
        range: R,
    ) -> Drain<Scheduled<Unreliable, DataTransfer>> {
        self.scheduler
            .list_scheduled_unreliable_data_transfer
            .drain(range)
    }
}
