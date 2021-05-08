use std::{net::SocketAddr, time::Duration};

use hecs::Entity;
use log::debug;

use crate::{
    events::{
        AckLocalPacketEvent, AckRemotePacketEvent, QueuedPacketEvent,
        ReceivedConnectionAcceptedEvent, ReceivedConnectionDeniedEvent, ReceivedDataTransferEvent,
        ReceivedHeartbeatEvent, ReceivedNewPacketEvent, SentConnectionRequestEvent,
        SentDataTransferEvent, SentHeartbeatEvent, SentPacketEvent,
    },
    host::{
        Address, AwaitingConnectionAck, Connected, Disconnected, LatestReceived, LatestSent,
        RequestingConnection, StateEnteredTime,
    },
    net::NetManager,
    packet::{
        header::Header, ConnectionId, ConnectionRequest, DataTransfer, Footer, Internal, Packet,
        Payload, Queued, Received, Retrieved, Sent, Sequence, CONNECTION_ACCEPTED,
        CONNECTION_DENIED, CONNECTION_REQUEST, DATA_TRANSFER, HEARTBEAT,
    },
    receiver::Source,
    sender::{Destination, RawPacket},
};

/// TODO(alex) 2021-05-01: Consider adding a `DebugName` component for every entity, such as when
/// doing `world.spawn((format!("Packet {:?}", header), components...))`.

/// TODO(alex) 2021-02-26: References for ideas about connection:
/// http://www.tcpipguide.com/free/t_PPPLinkSetupandPhases.htm
///
/// ADD(alex) 2021-02-26: We need a `Disconnecting` state (link termination phase)?
///
/// TODO(alex) 2021-03-04: Client and Server are different beasts right now, I'm thinking about
/// ways of allowing some sort of peer-to-peer communication, so a `Client` would have to track
/// connection (`Host<State>`) for multiple other clients. To to this we would need something
/// that looks more like the `Server`, and some way to keep one node of the network as the main
/// server? This idea is not clear yet.
pub struct Client {}

/// TODO(alex) 2021-04-24: `LatestReceived` makes sense to be part of the `Host` entity, this
/// way we can do `world.insert` and not have to remove and then insert (packet case).
#[derive(Debug)]
pub(crate) struct LastReceived {
    packet_id: Entity,
}

#[derive(Debug)]
pub(crate) struct LastSent {
    packet_id: Entity,
}

#[derive(Debug)]
pub(crate) struct LastAcked {
    packet_id: Entity,
}

impl NetManager<Client> {
    pub fn new_client(address: &SocketAddr) -> Self {
        let client = Client {};
        let net_manager = NetManager::new(address, client);
        net_manager
    }

    /// TODO(alex) 2021-02-26: Authentication is something that we can't do here, it's up to the
    /// user, but there must be an API for forcefully dropping a connection, plus banning a host.
    pub fn connect(&mut self, server_addr: &SocketAddr) -> () {
        let world = &mut self.world;

        // TODO(alex) 2021-04-03: Check for existing hosts in different states, we do nothing
        // if the host is already `RequestingConnection, AwaitingConnectionAck, Connected`, we only
        // want to do something if `Disconnected` or non-existent.
        //
        // ADD(alex) 2021-04-03: This check is probably good enough.
        let existing_host_id = world
            .query::<(&Address,)>()
            .with::<Disconnected>()
            .iter()
            .find_map(|(host_id, (address,))| (address.0 == *server_addr).then_some(host_id));
        debug!(
            "Client::connect -> existing_host_id {:#?}",
            existing_host_id
        );

        // NOTE(alex) 2021-04-04: Can't combine this in the `find_map` because `query` does a
        // mutable borrow of `world`, so it doesn't allow `world.spawn` when using the
        // `unwrap_or_else` closure, even though the code should be completely equivalent.
        type Host = (Address, RequestingConnection, StateEnteredTime);
        let time = self.timer.elapsed();
        let host_id = existing_host_id.unwrap_or_else(|| {
            let host: Host = (
                Address(server_addr.clone()),
                RequestingConnection { attempts: 0 },
                StateEnteredTime(time),
            );
            world.spawn(host)
        });
        debug!("Client::connect -> host_id {:#?}", host_id);

        let payload = Payload::default();

        let status_code = CONNECTION_REQUEST;
        let address = Address(server_addr.clone());
        type BasicPacket = (Payload, Address, Queued, ConnectionRequest, Destination);
        let packet: BasicPacket = (
            payload,
            address,
            Queued {
                time: self.timer.elapsed(),
            },
            ConnectionRequest,
            Destination { host_id },
        );
        let packet_id = world.spawn(packet);
        debug!("Client::connect -> spawning packet {:#?}", packet_id);

        let queued_packet_event = (QueuedPacketEvent {
            packet_id,
            status_code,
        },);
        let event_id = world.spawn(queued_packet_event);
        debug!("Client::connect -> spawning `SendPacket` {:#?}", event_id);
    }

    pub fn enqueue(&mut self, message: Vec<u8>) {
        debug!("Client::enqueue -> user requested packet enqueue");
        let mut enqueue_packet = None;

        if let Some((host_id, (address, connected))) =
            self.world.query::<(&Address, &Connected)>().iter().last()
        {
            debug!(
                "Client::enqueue -> connected host {:?} {:?}",
                host_id, address
            );
            let payload = Payload(message);

            type BasicPacket = (Payload, Address, Queued, DataTransfer, Destination);
            let packet: BasicPacket = (
                payload,
                address.clone(),
                Queued {
                    time: self.timer.elapsed(),
                },
                DataTransfer,
                Destination { host_id },
            );

            enqueue_packet = Some(packet);
        }

        if let Some(packet) = enqueue_packet {
            let packet_id = self.world.spawn(packet);

            let queued_packet_event = (QueuedPacketEvent {
                packet_id,
                status_code: DATA_TRANSFER,
            },);
            let event_id = self.world.spawn(queued_packet_event);
            debug!(
                "Client::enqueue -> spawning PreparePacketToSendEvent {:?}",
                event_id
            );
        }
    }

    pub fn connected(&mut self) {}

    pub fn denied(&mut self) {
        todo!()
    }

    /// TODO(alex) 2021-02-23: Return some indication that the manager received new packets and the
    /// user should call `retrieve`.
    /// ADD(alex) 2021-02-26: The return can be made even more general, by having an enum of
    /// possibilities, like `HasMessagesToRetrieve`, `ConnectionLost`. The only success cases I can
    /// think of are `HasMessagesToRetrieve` and `NothingToReport`? But the errors are plenty, like
    /// `ReceivingMessageFromBannedHost`, `FailedToSend`, `FailedToReceive`, `FailedToEncode`, ...
    pub fn tick(&mut self) -> () {
        self.check_readiness();
        self.receiver();
        self.on_received_new_packet();
        self.on_received_connection_accepted();
        self.on_received_connection_denied();
        self.on_queued_packet();
        self.sender();
        self.on_sent_packet();
        self.on_sent_connection_request();

        // TODO(alex) 2021-05-05: Check if there are packets to ack here, and maybe send a heartbeat
        // back to the server, just to ack the packet + confirm the connection was estabilished.
        // This query will look for enqueued packets, if there are none, then send a heartbeat.
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

        let mut known_host_packets = Vec::with_capacity(8);
        let mut invalid_packets = Vec::with_capacity(8);

        for (event_id, (event,)) in world.query::<(&ReceivedNewPacketEvent,)>().iter() {
            debug!(
                "Client::on_received_new_packet handle ReceivedNewPacket {:#?}",
                event_id
            );

            let packet_id = event.packet_id;

            let mut packet_query = world
                .query_one::<(&Sequence, &Header, &Footer, &Address)>(packet_id)
                .unwrap();
            let (sequence, header, footer, address) = packet_query.get().unwrap();
            debug!(
                "Client::on_received_new_packet packet {:#?} info {:#?} {:#?} {:#?}",
                packet_id, header, footer, address
            );

            // NOTE(alex): Check if there is an existing host with this address.
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
                    "Client::on_received_new_packet packet belongs to a known host {:#?} ",
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
            } else {
                debug!("Client::on_received_new_packet packet is invalid (not connection request)");
                // NOTE(alex): `Source`less packets can only be connection requests.
                invalid_packets.push(packet_id);
            }

            handled_events.push(event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
            debug!(
                "Client::on_received_new_packet despawning ReceivedNewPacket {:#?}",
                event_id
            );
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
                (CONNECTION_DENIED, None) => {
                    let event_id = world.spawn((ReceivedConnectionDeniedEvent {
                        packet_id,
                        source_id: host_id,
                    },));
                    debug!(
                        "Client::on_received_new_packet spawning ReceivedConnectionDenied {:#?}",
                        event_id
                    );
                }
                (CONNECTION_ACCEPTED, Some(_)) => {
                    let event_id = world.spawn((ReceivedConnectionAcceptedEvent {
                        packet_id,
                        source_id: host_id,
                    },));
                    debug!(
                        "Client::on_received_new_packet spawning ReceivedConnectionAccepted {:#?}",
                        event_id
                    );
                }
                (DATA_TRANSFER, Some(_)) => {
                    let event_id = world.spawn((ReceivedDataTransferEvent {
                        packet_id,
                        source_id: host_id,
                    },));
                    debug!(
                        "Client::on_received_new_packet spawning ReceivedDataTransfer {:#?}",
                        event_id
                    );
                }
                (HEARTBEAT, Some(_)) => {
                    let event_id = world.spawn((ReceivedHeartbeatEvent {
                        packet_id,
                        source_id: host_id,
                    },));
                    debug!(
                        "Client::on_received_new_packet spawning ReceivedHeartbeat {:#?}",
                        event_id
                    );
                }
                invalid => {
                    eprintln!(
                        "Client::on_received_new_packet invalid packet type {:#?}.",
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
                    "Client::on_received_new_packet spawning AckLocalPacket {:#?}",
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
                "Client::on_received_new_packet spawning AckRemotePacket {:#?}",
                event_id
            );
        }

        while let Some(packet_id) = invalid_packets.pop() {
            let _ = world.despawn(packet_id).unwrap();
            debug!(
                "Client::on_received_new_packet despawning invalid packet {:#?}",
                packet_id
            );
        }
    }

    /// TODO(alex) 2021-03-07: Think of how network libraries usually have a `listen` function,
    /// instead of manually calling `receive`.
    /// NOTE(alex): This is less of a system, and more just a function that the user will call, part
    /// of the public API (exposed via `NetManager` client / server).
    pub fn retrieve(&mut self) -> Vec<(ConnectionId, Vec<u8>)> {
        let world = &mut self.world;

        let mut data_transfers = Vec::with_capacity(8);
        let mut result = Vec::with_capacity(8);
        let mut invalid_packets = Vec::with_capacity(8);

        for (packet_id, (header, payload, footer, source, received)) in world
            .query::<(&Header, &Payload, &Footer, &Source, &Received)>()
            .with::<DataTransfer>()
            .without::<Retrieved>()
            .iter()
        {
            let host_query = world.query_one::<&Address>(source.host_id).unwrap();
            // NOTE(alex) 2021-04-09: This is the main difference between `Client` and `Server`, as
            // the client will only accept `DataTransfer`s after the connection is estabilished, but
            // the server will receive a `DataTransfer` while the `Host` is still in the
            // `AwaitingAck`-sort of state, this packet will ack the `ConnectionAccepted` packet
            // that the server sent previously, and estabilish the connection.
            if let Some(_) = host_query.with::<Connected>().get() {
                data_transfers.push(packet_id);
                result.push((footer.connection_id.unwrap(), payload.0.clone()));
            } else {
                invalid_packets.push(packet_id);
            }
        }

        while let Some(packet_id) = data_transfers.pop() {
            let _ = world
                .insert(
                    packet_id,
                    (Retrieved {
                        time: self.timer.elapsed(),
                    },),
                )
                .unwrap();
        }

        while let Some(packet_id) = invalid_packets.pop() {
            let _ = world.despawn(packet_id).unwrap();
        }

        result
    }

    pub(crate) fn on_received_connection_accepted(&mut self) {
        let world = &mut self.world;

        let mut handled_events = Vec::with_capacity(8);
        let mut connected_hosts = Vec::with_capacity(8);
        let mut invalid_packets = Vec::with_capacity(8);

        for (event_id, event) in world.query::<&ReceivedConnectionAcceptedEvent>().iter() {
            debug!(
                "Client::on_received_connection_accepted handle ReceivedConnectionAccepted {:#?}",
                event_id
            );

            let mut packet_query = world
                .query_one::<(&Header, &Footer, &Source)>(event.packet_id)
                .unwrap();
            let (header, footer, source) = packet_query.get().unwrap();

            debug_assert_eq!(source.host_id, event.source_id);
            debug_assert!(footer.connection_id.is_some());

            let mut host_query = world
                .query_one::<&AwaitingConnectionAck>(source.host_id)
                .unwrap();
            match host_query.get() {
                Some(_) => {
                    connected_hosts.push((
                        source.host_id,
                        event.packet_id,
                        footer.connection_id.unwrap(),
                    ));
                }
                None => {
                    eprintln!(
                        "Host is in an invalid state to accept this packet {:#?}.",
                        header
                    );
                    invalid_packets.push(event.packet_id);
                }
            }

            handled_events.push(event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
        }

        while let Some((source_id, packet_id, connection_id)) = connected_hosts.pop() {
            let (_awaiting_connection_ack,) =
                world.remove::<(AwaitingConnectionAck,)>(source_id).unwrap();
            let _ = world
                .insert(
                    source_id,
                    (
                        Connected {
                            connection_id,
                            rtt: Duration::default(),
                        },
                        StateEnteredTime(self.timer.elapsed()),
                    ),
                )
                .unwrap();

            // TODO(alex): Mark this packet as handled, somehow.
            world
                .insert(
                    packet_id,
                    (Internal {
                        time: self.timer.elapsed(),
                    },),
                )
                .unwrap();
        }

        while let Some(packet_id) = invalid_packets.pop() {
            world.despawn(packet_id).unwrap();
            debug!("Client::on_received_connection_accepted despawning ReceivedConnectionAccepted");
        }
    }

    pub(crate) fn on_received_connection_denied(&mut self) {
        let (buffer, world, timer) = (&mut self.buffer, &mut self.world, &self.timer);

        let mut handled_events = Vec::with_capacity(8);

        let mut denied_hosts = Vec::with_capacity(8);
        let mut invalid_packets = Vec::with_capacity(8);

        for (event_id, event) in world.query::<&ReceivedConnectionDeniedEvent>().iter() {
            let mut packet_query = world
                .query_one::<(&Header, &Source)>(event.packet_id)
                .unwrap();
            let (header, source) = packet_query.get().unwrap();
            debug_assert_eq!(source.host_id, event.source_id);

            let mut host_query = world
                .query_one::<&AwaitingConnectionAck>(source.host_id)
                .unwrap();
            // NOTE(alex): Only hosts with `AwaitingConnectionAck` may handle this type of packet.
            match host_query.get() {
                Some(_) => {
                    denied_hosts.push((source.host_id, event.packet_id));
                }
                None => {
                    eprintln!(
                        "Host is in an invalid state to accept this packet {:#?}.",
                        header
                    );
                    invalid_packets.push(event.packet_id);
                }
            }
            handled_events.push(event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
        }

        while let Some((source_id, packet_id)) = denied_hosts.pop() {
            let (_awaiting_connection_ack,) =
                world.remove::<(AwaitingConnectionAck,)>(source_id).unwrap();
            let _ = world
                .insert(
                    source_id,
                    (
                        Disconnected { x: 0 },
                        StateEnteredTime(self.timer.elapsed()),
                    ),
                )
                .unwrap();
            // TODO(alex): Mark this packet as handled, somehow.
            world
                .insert(
                    packet_id,
                    (Internal {
                        time: self.timer.elapsed(),
                    },),
                )
                .unwrap();
        }

        while let Some(packet_id) = invalid_packets.pop() {
            let _ = world.despawn(packet_id).unwrap();
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
                CONNECTION_REQUEST => {
                    let event_id = world.spawn((SentConnectionRequestEvent { packet_id },));
                    debug!("sender -> spawning SentConnectionRequest {:#?}", event_id);
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

    pub(crate) fn on_sent_connection_request(&mut self) {
        let world = &mut self.world;

        let mut handled_events = Vec::with_capacity(8);
        let mut awaiting_connection_ack_hosts = Vec::with_capacity(8);

        for (event_id, event) in world.query::<&SentConnectionRequestEvent>().iter() {
            let mut destination_query = world.query_one::<&Destination>(event.packet_id).unwrap();
            let destination = destination_query.get().unwrap();

            awaiting_connection_ack_hosts.push(destination.host_id);
            handled_events.push(event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
        }

        while let Some(host_id) = awaiting_connection_ack_hosts.pop() {
            // TODO(alex) 2021-05-01: There must be someplace ensuring that this kind of packet is
            // only sent from `RequestingConnection` hosts, or this invariant will fail.
            let (requesting_connection,) =
                world.remove::<(RequestingConnection,)>(host_id).unwrap();
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
        }
    }

    pub(crate) fn poll_requesting_connection(&mut self) {
        // TODO(alex) 2021-05-02: Check if the connection timed out, and resend the connection
        // request, if the number of attempts is still valid.
        let world = &mut self.world;

        for (host_id, (requesting_connection, address)) in
            world.query::<(&RequestingConnection, &Address)>().iter()
        {}
    }

    pub(crate) fn poll_awaiting_connection_ack(&mut self) {
        // TODO(alex) 2021-05-02: Check if the connection timed out, and resend the connection
        // request, if the number of attempts is still valid.
    }

    /// TODO(alex) 2021-02-28: Returns the `Packet<ToSend>::Header::sequence` value so that the user
    /// may use it to remove the packet (if wanted). It'll be an API for users that might want to
    /// clear older packets that were never sent, and to allow users to check for packets that were
    /// actually sent.
    ///
    /// ADD(alex) 2021-03-03: This function is `async` when I think about it. The whole idea of
    /// queueing the packets to send, instead of sending them directly comes from the need to check
    /// if the `socket` is writable, so we enqueue the packets instead of just calling
    /// `socket.send` here, creating an async version of `socket.send` basically. `Client::tick`
    /// is very similar to what I think `poll` would look like.
    ///
    /// Using `Future` would simplify the API, as it would remove the need for the `Client::tick`
    /// function, so `Client::connect -> Future<ConnectedClient>` would do the connection handling
    /// that `Client::tick` is doing, while `Client::send` and `Client::receive` would do the rest
    /// (data transfer handling). `Client::receive` acts like the "update" function, as it will be
    /// called all the time (with some user tickrate), being equivalent to a "listen" function.
    pub fn send(&mut self, data: Vec<u8>) -> () {
        todo!()
    }

    pub fn send_priority(&self, data: Vec<u8>) -> () {
        todo!();
    }
}
