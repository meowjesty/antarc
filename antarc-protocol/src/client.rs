use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
};

use log::debug;

use crate::{
    events::{AntarcEvent, ProtocolError},
    packets::*,
    peers::{AwaitingConnectionAck, Connected, Peer, RequestingConnection},
    EventSystem, Protocol,
};

#[derive(Debug)]
pub struct Client {
    pub last_sent_time: Duration,
    pub requesting_connection: HashMap<ConnectionId, Peer<RequestingConnection>>,
    pub awaiting_connection_ack: HashMap<ConnectionId, Peer<AwaitingConnectionAck>>,
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

    /// TODO(alex) [high] 2021-08-01: Steps to do here:
    /// 1. check if this address is already in some of the client's state;
    /// 2. create a new `Peer<RequestingConnection>`;
    /// 3. schedule a connection request `Scheduled` via some kind of event (must be `Reliable`);
    pub fn connect(&mut self, remote_address: SocketAddr) {
        todo!()
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

                self.events.scheduler.push(scheduling);
            }
        }

        if self.events.scheduler.len() > old_scheduler_length {
            self.packet_id_tracker += 1;
        }
    }
}
