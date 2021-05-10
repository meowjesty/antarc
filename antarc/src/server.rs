use std::{
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
    time::{Duration, Instant},
};

use hecs::World;
use log::{debug, error};

use crate::{
    events::{
        AckLocalPacketEvent, AckRemotePacketEvent, QueuedPacketEvent,
        ReceivedConnectionAcceptedEvent, ReceivedConnectionDeniedEvent,
        ReceivedConnectionRequestEvent, ReceivedDataTransferEvent, ReceivedHeartbeatEvent,
        ReceivedNewPacketEvent, SentConnectionAcceptedEvent, SentConnectionDeniedEvent,
        SentConnectionRequestEvent, SentDataTransferEvent, SentHeartbeatEvent, SentPacketEvent,
    },
    host::{
        Address, AwaitingConnectionAck, Disconnected, LatestReceived, LatestSent,
        RequestingConnection, StateEnteredTime,
    },
    net::{NetManager, NetworkResource},
    packet::{
        header::Header, ConnectionAccepted, ConnectionId, Footer, Payload, Queued, Sent, Sequence,
        CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST, DATA_TRANSFER, HEARTBEAT,
    },
    receiver::Source,
    sender::Destination,
};

#[derive(Debug)]
pub struct Server {
    connection_id_tracker: ConnectionId,
}

impl NetManager<Server> {
    pub fn new_server(address: &SocketAddr) -> Self {
        let server = Server {
            connection_id_tracker: unsafe { ConnectionId::new_unchecked(1) },
        };
        let net_manager = NetManager::new(address, server);
        net_manager
    }

    /// TODO(alex) 2021-03-08: We need an API like `get('/{:id}')` route, but for `Host`s.
    pub fn listen(&mut self) -> () {
        todo!()
    }

    pub fn tick(&mut self) -> () {
        self.network_resource
            .poll
            .poll(
                &mut self.network_resource.events,
                Some(Duration::from_millis(5)),
            )
            .unwrap();

        let mut sender_result = None;
        let mut receiver_result = None;

        for event in self.network_resource.events.iter().clone() {
            match event.token() {
                NetworkResource::TOKEN => {
                    if event.is_readable() {
                        receiver_result = Some(self.receiver(&mut self.buffer.clone()));
                    }

                    if event.is_writable() {
                        sender_result = Some(self.sender());
                    }
                }
                _ => unreachable!(),
            }
        }

        if let Some((mut sent_packets, mut handled_events)) = sender_result {
            while let Some(queued_event_id) = handled_events.pop() {
                let _ = self.world.despawn(queued_event_id).unwrap();
                debug!(
                    "{} {}:{} -> despawning handled QueuedPacketEvent {:#?}",
                    file!(),
                    line!(),
                    column!(),
                    queued_event_id
                );
            }

            while let Some((sequence, header, packet_id, destination)) = sent_packets.pop() {
                let status_code = header.status_code;
                self.world
                    .insert(
                        packet_id,
                        (
                            sequence,
                            header,
                            Sent {
                                time: self.timer.elapsed(),
                            },
                        ),
                    )
                    .unwrap();
                debug!(
                    "{} {}:{} -> insert sequence header sent into packet_id {:#?}",
                    file!(),
                    line!(),
                    column!(),
                    packet_id
                );

                // TODO(alex) 2021-05-02: Check if this packet has sequence > previous latest sent.
                self.world
                    .insert(destination.host_id, (LatestSent { packet_id },))
                    .unwrap();
                debug!(
                    "{} {}:{} -> insert `LatestSent` into host_id {:#?}",
                    file!(),
                    line!(),
                    column!(),
                    destination.host_id
                );

                let event_id = self.world.spawn((SentPacketEvent {
                    packet_id,
                    status_code,
                },));
                debug!(
                    "{} {}:{} -> spawn SentPacketEvent {:#?}",
                    file!(),
                    line!(),
                    column!(),
                    event_id
                );
            }
        }

        self.on_received_new_packet();
        self.on_received_connection_request();
        self.on_sent_connection_accepted();
        self.on_sent_packet();
    }

    /// System responsible for attributing a `Source` host to a packet entity, if the host matches
    /// the packet's `Address`, otherwise it raises the `OnReceivedConnectionRequest` event (if
    /// the `Header` represents a `ConnectionRequest`).
    ///
    /// Also handles the changes to a host's `LatestReceived` packet.
    ///
    /// - Raises the `OnReceivedConnectionRequest` event.
    pub(crate) fn on_received_new_packet(&mut self) {
        let world = &mut self.world;

        let mut handled_events = Vec::with_capacity(8);

        let mut unknown_host_packets = Vec::with_capacity(8);
        let mut known_host_packets = Vec::with_capacity(8);
        let mut invalid_packets = Vec::with_capacity(8);

        for (event_id, (event,)) in world.query::<(&ReceivedNewPacketEvent,)>().iter() {
            debug!(
                "Server::on_received_new_packet handle ReceivedNewPacket {:#?}",
                event_id
            );

            let packet_id = event.packet_id;

            let mut packet_query = world
                .query_one::<(&Sequence, &Header, &Footer, &Address)>(packet_id)
                .unwrap();
            let (sequence, header, footer, address) = packet_query.get().unwrap();
            debug!(
                "Server::on_received_new_packet packet {:#?} info {:#?} {:#?} {:#?}",
                packet_id, header, footer, address
            );

            // NOTE(alex): Check if this packet has a `Source`.
            if let Some(host_id) = world
                .query::<(&Address, &StateEnteredTime)>() // both host and packet archetypes have this
                .iter()
                .find_map(|(host_id, (host_address, _))| {
                    if host_address == address {
                        Some(host_id)
                    } else {
                        None
                    }
                })
            {
                debug!(
                    "Server::on_received_new_packet packet belongs to a known host {:#?} ",
                    host_id
                );
                // NOTE(alex): Get the current `LatestReceived` packet for this host, to swap it
                // out.
                //
                // TODO(alex) 2021-04-22: The `new_sequence > old_sequence` check avoids the case
                // where a packet arrives out of order, and should not be marked as the latest, but
                // this won't hold if sequence wraps.
                let mut latest_received_query =
                    world.query_one::<(&LatestReceived,)>(host_id).unwrap();
                let latest = latest_received_query
                    .get()
                    .map_or(false, |(latest_received,)| {
                        let mut packet_query = world
                            .query_one::<(&Sequence,)>(latest_received.packet_id)
                            .unwrap();
                        let (old_sequence,) = packet_query.get().unwrap();
                        sequence > old_sequence
                    });
                known_host_packets.push((
                    packet_id,
                    host_id,
                    latest,
                    header.clone(),
                    footer.connection_id,
                ));
            } else if header.status_code == CONNECTION_REQUEST && footer.connection_id.is_none() {
                debug!("Server::on_received_new_packet packet belongs to an unknown host ");
                // NOTE(alex): `Source`less packet is a connection request, this is ok.
                debug_assert_eq!(header.status_code, CONNECTION_REQUEST);
                unknown_host_packets.push((packet_id, address.clone(), header.clone()));
            } else {
                debug!("Server::on_received_new_packet packet is invalid");
                // NOTE(alex): `Source`less packets can only be connection requests.
                invalid_packets.push(packet_id);
            }

            handled_events.push(event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
            debug!(
                "Server::on_received_new_packet despawning ReceivedNewPacket {:#?}",
                event_id
            );
        }

        while let Some((packet_id, address, header)) = unknown_host_packets.pop() {
            // WARNING(alex): Be very careful when using tupled types like this + `world.spawn`, if
            // you add up an extra pair of `()`, it becomes a tuple of tuples, which breaks down
            // the queries.
            type NewHost = (Address, StateEnteredTime, Disconnected);
            let new_host: NewHost = (
                address,
                StateEnteredTime(self.timer.elapsed()),
                Disconnected { x: 0 },
            );
            debug!("Server::on_received_new_packet new_host {:#?}", new_host);

            let host_id = world.spawn(new_host);
            debug!(
                "Server::on_received_new_packet spawning new host {:?}",
                host_id
            );

            known_host_packets.push((packet_id, host_id, true, header, None));
        }

        while let Some((packet_id, host_id, latest, header, connection_id)) =
            known_host_packets.pop()
        {
            // WARNING(alex): rust and rust-analyzer won't give an error if you forget to import the
            // constants that are to be `match`ed, instead it will do the normal destructuring
            // behaviour, and short-circuit whatever comes after the first non-imported name!
            // rust-analyzer will at least squiggle it suggesting that you use lower-case instead of
            // all-caps, as it thinks you're just creating a binding.
            match (header.status_code, connection_id) {
                (CONNECTION_REQUEST, None) => {
                    let event_id = world.spawn((ReceivedConnectionRequestEvent {
                        packet_id,
                        source_id: host_id,
                    },));
                    debug!(
                        "Server::on_received_new_packet spawning ReceivedConnectionRequest {:#?}",
                        event_id
                    );
                }
                (DATA_TRANSFER, Some(_)) => {
                    let event_id = world.spawn((ReceivedDataTransferEvent {
                        packet_id,
                        source_id: host_id,
                    },));
                    debug!(
                        "Server::on_received_new_packet spawning ReceivedDataTransfer {:#?}",
                        event_id
                    );
                }
                (HEARTBEAT, Some(_)) => {
                    let event_id = world.spawn((ReceivedHeartbeatEvent {
                        packet_id,
                        source_id: host_id,
                    },));
                    debug!(
                        "Server::on_received_new_packet spawning ReceivedHeartbeat {:#?}",
                        event_id
                    );
                }
                invalid => {
                    eprintln!(
                        "Server::on_received_new_packet invalid packet type received {:#?}.",
                        invalid
                    );
                    let _ = world.despawn(packet_id).unwrap();
                    unreachable!();
                }
            };

            let _ = world.insert(packet_id, (Source { host_id },)).unwrap();

            if latest {
                let _ = world
                    .insert(host_id, (LatestReceived { packet_id },))
                    .unwrap();
            }

            if header.ack != 0 {
                let event_id = world.spawn((AckLocalPacketEvent {
                    packet_id,
                    source_id: host_id,
                },));
                debug!(
                    "Server::on_received_new_packet spawning AckLocalPacket {:#?}",
                    event_id
                );
            }

            // TODO(alex) 2021-04-14: Raise event that happens after the `OnReceivedAckSentPacket`,
            // such as `OnReceivedAddToAck` (or some similar name). This event and system will be
            // responsible for adding a `ToAck` (or some sort) component to packets that are sent
            // back (responses).
            //
            // ADD(alex) 2021-04-14: This is the event, it's being raised both here, and on the
            // connection request handler. Here it's raised only if we have a `Source` already,
            // meanwhile the connection request handler raises it after adding a `Source`.
            let event_id = world.spawn((AckRemotePacketEvent {
                packet_id,
                destination_id: host_id,
            },));
            debug!(
                "Server::on_received_new_packet spawning AckRemotePacket {:#?}",
                event_id
            );
        }

        while let Some(packet_id) = invalid_packets.pop() {
            let _ = world.despawn(packet_id).unwrap();
            debug!(
                "Server::on_received_new_packet despawning invalid packet {:#?}",
                packet_id
            );
        }
    }

    // NOTE(alex): This handler is specific for the server, as the client doesn't receive connection
    // requests, at least not yet.
    pub(crate) fn on_received_connection_request(&mut self) {
        let world = &mut self.world;

        let mut handled_events = Vec::with_capacity(8);
        let mut connecting_hosts = Vec::with_capacity(8);
        let mut invalid_packets = Vec::with_capacity(8);

        for (event_id, event) in world.query::<&ReceivedConnectionRequestEvent>().iter() {
            debug!(
                "{} {}:{} -> handle ReceivedConnectionRequestEvent {:#?}",
                file!(),
                line!(),
                column!(),
                event_id
            );

            let mut packet_query = world
                .query_one::<(&Address,)>(event.packet_id)
                .unwrap()
                .with::<Header>();
            let (address,) = packet_query.get().unwrap();
            debug!(
                "{} {}:{} -> packet id {:#?} address {:#?} source {:#?}",
                file!(),
                line!(),
                column!(),
                event.packet_id,
                address,
                event.source_id
            );

            // NOTE(alex): New hosts are created by the `on_received_new_packet` event handler, so
            // they always arrive here with a `Source`. This checks if such a `Source` is in a valid
            // state to receive a `ConnectionRequest`.
            if let Some((address, disconnected)) = world
                .query_one::<(&Address, &Disconnected)>(event.source_id)
                .unwrap()
                .get()
            {
                connecting_hosts.push((event.packet_id, event.source_id, address.clone()));
                debug!(
                    "{} {}:{} -> host is disconnected",
                    file!(),
                    line!(),
                    column!()
                );
            } else {
                // NOTE(alex): Host is in an incompatible state to receive this kind of
                // packet.
                invalid_packets.push(event.packet_id);
                error!(
                    "{} {}:{} -> host is in invalid state",
                    file!(),
                    line!(),
                    column!()
                );
            }

            handled_events.push(event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
            debug!(
                "{} {}:{} -> despawning ReceivedConnectionRequestEvent {:#?}",
                file!(),
                line!(),
                column!(),
                event_id
            );
        }

        while let Some(packet_id) = invalid_packets.pop() {
            let _ = world.despawn(packet_id).unwrap();
            debug!(
                "{} {}:{} -> despawning invalid packet {:#?}",
                file!(),
                line!(),
                column!(),
                packet_id
            );
        }

        while let Some((packet_id, host_id, address)) = connecting_hosts.pop() {
            let (disconnected,) = world.remove::<(Disconnected,)>(host_id).unwrap();
            let requesting_connection = RequestingConnection { attempts: 0 };
            let state_entered_time = StateEnteredTime(self.timer.elapsed());
            debug!(
                "{} {}:{} -> inserting {:#?} {:#?} into host {:#?}",
                file!(),
                line!(),
                column!(),
                requesting_connection,
                state_entered_time,
                host_id
            );
            let _ = world
                .insert(host_id, (requesting_connection, state_entered_time))
                .unwrap();

            let payload = Payload::default();
            let status_code = CONNECTION_ACCEPTED;

            let queued = Queued {
                time: self.timer.elapsed(),
            };
            let connection_accepted = ConnectionAccepted;
            let destination = Destination { host_id };
            let connection_id = ConnectionId::new(1).unwrap();
            let queued_packet = (
                payload,
                address.clone(),
                queued,
                connection_accepted,
                destination,
                connection_id,
            );
            debug!(
                " {} {}:{} -> spawning queued packet {:#?}",
                file!(),
                line!(),
                column!(),
                queued_packet
            );

            let queued_packet_id = world.spawn(queued_packet);

            let queued_packet_event = QueuedPacketEvent {
                status_code: CONNECTION_ACCEPTED,
                packet_id: queued_packet_id,
            };
            debug!(
                "{} {}:{} -> spawning QueuedPacketEvent {:#?}",
                file!(),
                line!(),
                column!(),
                queued_packet_event
            );
            let event_id = world.spawn((queued_packet_event,));
        }
    }

    pub(crate) fn on_sent_packet(&mut self) {
        let world = &mut self.world;

        let mut handled_events = Vec::with_capacity(8);
        let mut sent_packets = Vec::with_capacity(8);

        for (event_id, event) in world.query::<&SentPacketEvent>().iter() {
            handled_events.push(event_id);
            sent_packets.push(event.clone());
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
            debug!(
                "sender -> despawning handled SentPacketEvent {:#?}",
                event_id
            );
        }

        while let Some(event) = sent_packets.pop() {
            let SentPacketEvent {
                packet_id,
                status_code,
            } = event;

            match status_code {
                CONNECTION_ACCEPTED => {
                    let event_id = world.spawn((SentConnectionAcceptedEvent { packet_id },));
                    debug!("sender -> spawning SentConnectionAccepted {:#?}", event_id);
                }
                CONNECTION_DENIED => {
                    let event_id = world.spawn((SentConnectionDeniedEvent { packet_id },));
                    debug!("sender -> spawning SentConnectionDenied {:#?}", event_id);
                }
                DATA_TRANSFER => {
                    let event_id = world.spawn((SentDataTransferEvent { packet_id },));
                    debug!("sender -> spawning SentDataTransferEvent {:#?}", event_id);
                }

                HEARTBEAT => {
                    let event_id = world.spawn((SentHeartbeatEvent { packet_id },));
                    debug!("sender -> spawning SentHeartbeatEvent {:#?}", event_id);
                }
                invalid => {
                    eprintln!("sender -> invalid packet type sent {:#?}.", invalid);
                    let _ = world.despawn(packet_id).unwrap();
                    unreachable!();
                }
            }
        }
    }

    // TODO(alex) 2021-04-27: Handle the `RequestingConnection` host state, we get the connection
    // request, create a host (if one does not exist with the same address already), but nothing is
    // being done to actually send back a connection accepted (or denied). I should start handling
    // only the accepted case, leave the option to deny a connection to later. I'm thinking about
    // this connection handler mechanism as being a simple syn/ack, and leaving proper connection
    // management to the user, so a connection denied would only be sent if a host belongs to a ban
    // list that the user has created. This means that the first time the connection will always be
    // replied with accepted.

    pub(crate) fn on_sent_connection_accepted(&mut self) {
        let world = &mut self.world;

        let mut handled_events = Vec::with_capacity(8);
        let mut accepted_connection_hosts = Vec::with_capacity(8);

        for (event_id, (event,)) in world.query::<(&SentConnectionAcceptedEvent,)>().iter() {
            debug!(
                "Server::on_sent_connection_accepted handle SentConnectionAcceptedEvent {:#?}",
                event_id
            );

            let mut destination_query = world.query_one::<&Destination>(event.packet_id).unwrap();
            let destination = destination_query.get().unwrap();

            accepted_connection_hosts.push(destination.host_id);
            handled_events.push(event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
            debug!(
                "Server::on_sent_connection_accepted despawning SentConnectionAcceptedEvent {:#?}",
                event_id
            );
        }

        while let Some(host_id) = accepted_connection_hosts.pop() {
            // TODO(alex) 2021-05-01: There must be someplace ensuring that this kind of packet is
            // only sent to `RequestingConnection` hosts, or this invariant will fail.
            let (requesting_connection,) =
                world.remove::<(RequestingConnection,)>(host_id).unwrap();
            debug!(
                "Server::on_sent_connection_accepted remove RequestingConnection {:#?} {:#?}",
                requesting_connection, host_id
            );

            let _ = world
                .insert(
                    host_id,
                    (
                        AwaitingConnectionAck {
                            attempts: requesting_connection.attempts,
                        },
                        StateEnteredTime(self.timer.elapsed()),
                    ),
                )
                .unwrap();
            debug!(
                "Server::on_sent_connection_accepted insert AwaitingConnectionAck {:#?}",
                host_id
            );
        }
    }

    pub fn retrieve(&self) -> Vec<(u32, Vec<u8>)> {
        todo!();
    }

    // pub fn enqueue(&self, data: Vec<u8>) {
    //     todo!()
    // }

    // pub fn ban_host(&self, host_id: u32) {
    //     todo!();
    // }
}
