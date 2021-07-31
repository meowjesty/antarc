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
    pub requesting_connection: HashMap<ConnectionId, Peer<RequestingConnection>>,
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

    /// NOTE(alex): API function for scheduling connection accepted packets, called by the user.
    // TODO(alex) [high] 2021-07-31: The existence of this function means that the user must handle
    // an event of connection request of sorts. When the protocol receives a connection request, it
    // will create (or update) the peer, and put it into the correct HashMap, then an API event
    // must be generated, so that the user may accept or deny this peer's connection.
    pub fn accept_connection(&mut self, connection_id: ConnectionId) {
        if let Some(peer) = self.service.requesting_connection.get(&connection_id) {
            debug!("server: Accepting connection for {:#?}.", connection_id);
            // TODO(alex) [high] 2021-07-31: After the connection accepted packet is scheduled here,
            // when the packet is sent by the network manager, there must be a handler event for
            // some sort of `SentEvent`, where we take the `Peer` out of requesting connection, and
            // put it into the next state.
            //
            // This can't be done here, as we have no means of guaranteeing the packet will be sent.
            //
            // The big question here is: who should keep retrying to send the connection accepted
            // packet, the `RequestingConnection` or the `AwaitingConnectionAck`? If it's the latter
            // (`AwaitingConnectionAck`), then we can change state here.
            let message = ConnectionAccepted {
                meta: MetaMessage {
                    packet_type: CONNECTION_ACCEPTED,
                },
                connection_id,
            };
            let scheduled = Scheduled {
                packet_id: self.service.packet_id_tracker,
                address: peer.address,
                time: self.timer.elapsed(),
                reliability: Reliable {},
                message,
            };

            self.scheduler_pipe.push(scheduled.into());
            self.service.packet_id_tracker += 1;
        } else {
            self.events
                .api
                .push(ProtocolError::NotFound(connection_id).into());
        }
    }

    /// NOTE(alex): API function for scheduling data transfers only, called by the user.
    /// TODO(alex) [mid] 2021-07-31: This function is duplication-city, most of the code inside the
    /// `if should_fragment else` is a copy of each other.
    pub fn schedule(&mut self, reliable: bool, send_to: SendTo, payload: Payload) {
        let should_fragment = payload.len() > MAX_FRAGMENT_SIZE;

        match send_to {
            SendTo::Single { connection_id } => {
                debug!("server: SendTo::Single scheduler {:#?}.", connection_id);

                let old_scheduler_length = self.scheduler_pipe.len();

                if let Some(peer) = self.service.connected.get(&connection_id) {
                    let packet_id = self.service.packet_id_tracker;

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

                        self.scheduler_pipe.append(&mut scheduling);
                    } else {
                        debug!("server: schedule non-fragment.");

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

                        self.scheduler_pipe.push(scheduling);
                    }
                } else {
                    self.events
                        .api
                        .push(ProtocolError::ScheduleInvalidPeer(connection_id).into())
                }

                if self.scheduler_pipe.len() > old_scheduler_length {
                    self.service.packet_id_tracker += 1;
                }
            }
            SendTo::Multiple { connection_ids } => {
                debug!("server: SendTo::Multiple scheduler {:#?}.", connection_ids);

                let old_scheduler_length = self.scheduler_pipe.len();

                for connection_id in connection_ids {
                    if let Some(peer) = self.service.connected.get(&connection_id) {
                        let packet_id = self.service.packet_id_tracker;

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

                            self.scheduler_pipe.append(&mut scheduling);
                        } else {
                            debug!("server: schedule non-fragment.");

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

                            self.scheduler_pipe.push(scheduling);
                        }
                    } else {
                        self.events
                            .api
                            .push(ProtocolError::ScheduleInvalidPeer(connection_id).into())
                    }
                }

                if self.scheduler_pipe.len() > old_scheduler_length {
                    self.service.packet_id_tracker += 1;
                }
            }
            SendTo::Broadcast => {
                debug!("server: SendTo::Broadcast scheduler.");

                let old_scheduler_length = self.scheduler_pipe.len();

                for peer in self.service.connected.values() {
                    let packet_id = self.service.packet_id_tracker;

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
                                    connection_id: peer.connection.connection_id,
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

                        self.scheduler_pipe.append(&mut scheduling);
                    } else {
                        debug!("server: schedule non-fragment.");

                        let meta = MetaMessage {
                            packet_type: DATA_TRANSFER_FRAGMENTED,
                        };
                        let message = DataTransfer {
                            meta,
                            connection_id: peer.connection.connection_id,
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

                        self.scheduler_pipe.push(scheduling);
                    }
                }

                if self.scheduler_pipe.len() > old_scheduler_length {
                    self.service.packet_id_tracker += 1;
                }
            }
        }
    }

    pub fn connection_request_another_state(
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
                    if Protocol::connection_request_another_state(
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
                        .values_mut()
                        .find(|peer| peer.address == packet.delivery.meta.remote)
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

                        let connection_id = self.service.connection_id_tracker;
                        self.service
                            .requesting_connection
                            .insert(connection_id, new_peer);
                        self.service.connection_id_tracker =
                            ConnectionId::new(connection_id.get() + 1).unwrap();
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

                        peer.remote_ack_tracker = packet.sequence.get();
                        peer.local_ack_tracker = packet.ack;
                    } else if let Some(mut peer) =
                        self.service.awaiting_connection_ack.remove(&connection_id)
                    {
                        debug!("server: host is awaiting connection ack {:#?}.", peer);

                        peer.remote_ack_tracker = packet.sequence.get();
                        peer.local_ack_tracker = packet.ack;
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

                        peer.remote_ack_tracker = packet.sequence.get();
                        peer.local_ack_tracker = packet.ack;
                    } else if let Some(mut peer) =
                        self.service.awaiting_connection_ack.remove(&connection_id)
                    {
                        debug!("server: host is awaiting connection ack {:#?}.", peer);

                        peer.remote_ack_tracker = packet.sequence.get();
                        peer.local_ack_tracker = packet.ack;
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
