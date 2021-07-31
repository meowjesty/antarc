use std::{collections::HashMap, net::SocketAddr, time::Duration};

use log::{debug, warn};

use crate::{
    events::{AntarcEvent, ProtocolError, ReceiverEvent},
    packets::*,
    peers::{AwaitingConnectionAck, Connected, MetaConnection, Peer, RequestingConnection, SendTo},
    Protocol,
};

#[derive(Debug)]
pub struct Server {
    pub packet_id_tracker: PacketId,
    pub last_antarc_schedule_check: Duration,
    pub connection_id_tracker: ConnectionId,
    pub requesting_connection: HashMap<SocketAddr, Peer<RequestingConnection>>,
    pub awaiting_connection_ack: HashMap<ConnectionId, Peer<AwaitingConnectionAck>>,
    pub connected: HashMap<ConnectionId, Peer<Connected>>,
}

impl Protocol<Server> {
    pub fn new_server() -> Self {
        todo!()
    }

    /// NOTE(alex): API function that feeds the internal* event pipe, it's called from the outside.
    pub fn received(&mut self, raw_packet: RawPacket) {
        raw_packet.decode(&mut self.events);
    }

    pub fn schedule(&mut self, send_to: SendTo, payload: Payload) {
        // TODO(alex) [high] 2021-07-30: Steps to do here:
        // 1. check payload length and fragment it;
        // 2. create the correct type of scheduled event (fragmented or full):
        // 3. any errors possible here?
        // 4. handle reliable and unreliable scheduling;
        //
        // Does this mean we need a scheduled event for:
        // - reliable + fragmented;
        // - unreliable + fragmented;
        // - reliable + complete;
        // - unreliable + complete;
        //
        // Is there a better way?
        todo!()
    }

    pub fn connection_request_valid_state(
        awaiting_connection_ack: &HashMap<ConnectionId, Peer<AwaitingConnectionAck>>,
        connected: &HashMap<ConnectionId, Peer<Connected>>,
        packet: &Packet<Received, ConnectionRequest>,
    ) -> bool {
        let address = packet.delivery.meta.remote;
        awaiting_connection_ack
            .values()
            .any(|peer| peer.address == address)
            || connected.values().any(|peer| peer.address == address)
    }

    pub fn poll(&mut self) -> Vec<AntarcEvent> {
        for received in self.events.receiver.drain(..) {
            match received {
                ReceiverEvent::ConnectionRequest { packet } => {
                    debug!("server: received connection request {:#?}.", packet);
                    // NOTE(alex): A host only changes RequestingConnection -> AwaitingConnectionAck
                    // after a connection accepted/denied packet is sent.
                    if Protocol::connection_request_valid_state(
                        &self.service.awaiting_connection_ack,
                        &self.service.connected,
                        &packet,
                    ) {
                        warn!(
                            "server: host already in another state, skipping {:#?}.",
                            packet
                        );
                        continue;
                    }

                    if let Some(peer) = self
                        .service
                        .requesting_connection
                        .get_mut(&packet.delivery.meta.remote)
                    {
                        peer.connection.attempts += 1;
                    } else {
                        let new_peer = Peer {
                            sequence_tracker: Sequence::new(1).unwrap(),
                            remote_ack_tracker: 1,
                            local_ack_tracker: 0,
                            address: packet.delivery.meta.remote,
                            connection: RequestingConnection {
                                meta: MetaConnection {},
                                attempts: 0,
                            },
                        };

                        self.service
                            .requesting_connection
                            .insert(new_peer.address, new_peer);
                    }
                }
                ReceiverEvent::ConnectionAccepted { packet } => {
                    warn!(
                        "server: received connection accepted, skipping {:#?}.",
                        packet
                    );
                    continue;
                }
                ReceiverEvent::DataTransfer { packet } => {
                    debug!("server: received data transfer {:#?}.", packet);

                    let connection_id = packet.message.connection_id;
                    let payload = packet.message.payload;

                    if let Some(peer) = self.service.connected.get_mut(&connection_id) {
                        debug!("server: host is connected {:#?}.", peer);

                        peer.remote_ack_tracker = packet.message.sequence.get();
                        peer.local_ack_tracker = packet.message.ack;
                    } else if let Some(mut peer) =
                        self.service.awaiting_connection_ack.remove(&connection_id)
                    {
                        debug!("server: host is awaiting connection ack {:#?}.", peer);

                        peer.remote_ack_tracker = packet.message.sequence.get();
                        peer.local_ack_tracker = packet.message.ack;
                        let connected = peer.connected(connection_id);

                        self.service.connected.insert(connection_id, connected);

                        self.events.api.push(AntarcEvent::DataTransfer {
                            connection_id,
                            payload,
                        });
                    }
                }
                ReceiverEvent::Heartbeat { packet } => {
                    debug!("server: received heartbeat {:#?}.", packet);

                    let connection_id = packet.message.connection_id;

                    if let Some(peer) = self.service.connected.get_mut(&connection_id) {
                        debug!("server: host is connected {:#?}.", peer);

                        peer.remote_ack_tracker = packet.message.sequence.get();
                        peer.local_ack_tracker = packet.message.ack;
                    } else if let Some(mut peer) =
                        self.service.awaiting_connection_ack.remove(&connection_id)
                    {
                        debug!("server: host is awaiting connection ack {:#?}.", peer);

                        peer.remote_ack_tracker = packet.message.sequence.get();
                        peer.local_ack_tracker = packet.message.ack;
                        let connected = peer.connected(connection_id);

                        self.service.connected.insert(connection_id, connected);
                    }
                }
            }
        }

        // TODO(alex) [mid] 2021-07-30: Handle `scheduler` pipe of events. But how exactly?
        // The network will check if socket is ready, then call a `make_packet` that will take some
        // scheduled from the scheduler pipe, but how does it handle reliability?


        self.events.api.drain(..).collect()
    }

    pub fn on_received(&mut self, raw_packet: RawPacket) -> Result<(), ProtocolError> {
        unimplemented!()
    }
}
