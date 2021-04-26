use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use hecs::World;
use log::{debug, info};
use mio::{net::UdpSocket, Events, Poll};

use crate::{
    host::{
        Address, AwaitingConnectionAck, Connected, Disconnected, RequestingConnection,
        SendingConnectionRequest,
    },
    net::NetManager,
    packet::{
        header::Header, ConnectionId, ConnectionRequest, DataTransfer, Footer, Packet, Payload,
        Queued, Received, Retrieved, CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST,
        DATA_TRANSFER, HEARTBEAT,
    },
    receiver::{
        AckLocalPacket, AckRemotePacket, LatestReceived, ReceivedConnectionAccepted,
        ReceivedConnectionDenied, ReceivedConnectionRequest, ReceivedDataTransfer,
        ReceivedHeartbeat, ReceivedNewPacket, SendPacket, Source,
    },
    sender::{Destination, RawPacket},
    MTU_LENGTH,
};

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

impl NetManager<Client> {
    pub fn new_client(address: &SocketAddr) -> Self {
        let client = Client {};
        let net_manager = NetManager::new(address, client);
        net_manager
    }

    /// Creates a new `Host<Connecting>` or updates an existing `Host<Disconnected>`, then adds a
    /// connection request packet to this host's `to_send` list.
    /// TODO(alex) 2021-02-07: Handle the connection request to a server, there are a few points
    /// to consider:
    ///
    /// 1. Is it okay to connect to another client, when this one is already connected to a server?
    ///    - probably not, keep connected checks to empty or single element in the client;
    /// 2. Create connection request packet:
    ///    - must check if there is an outgoing request already, that has not yet been acked.
    ///    - this function should keep itself to packet handling only, do not try to escalate it
    ///    into handling packet loss or anything like that, this will be part of the network
    ///    implementation, as it requires reading incoming packets.
    ///
    /// TODO(alex) 2021-02-26: Authentication is something that we can't do here, it's up to the
    /// user, but there must be an API for forcefully dropping a connection, plus banning a host.
    ///
    /// TODO(alex) 2021-04-24: `LatestReceived` makes sense to be part of the `Host` entity, this
    /// way we can do `world.insert` and not have to remove adn then insert (packet case).
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
        // mutable borrow of `world`, so it doesn't allow `world.spawn`.
        let host_id = existing_host_id.unwrap_or_else(|| {
            let host_id = world.spawn((
                Address(server_addr.clone()),
                SendingConnectionRequest { attempts: 0 },
            ));
            host_id
        });
        debug!("Client::connect -> host_id {:#?}", host_id);

        let header = Header {
            status_code: CONNECTION_REQUEST,
            ..Default::default()
        };
        let payload = Payload(Vec::new());
        let (bytes, footer) = Packet::encode(&header, &payload, None).unwrap();
        debug!("Client::connect -> footer {:#?}", footer);

        let packet_id = world.spawn((
            header,
            payload,
            footer,
            Address(server_addr.clone()),
            Queued {
                time: self.timer.elapsed(),
            },
            ConnectionRequest,
            Destination { host_id },
        ));
        debug!(
            "Client::connect -> spawning packet with id: {:#?}",
            packet_id
        );

        let raw_packet_id = world.spawn((RawPacket {
            packet_id,
            bytes,
            address: server_addr.clone(),
        },));
        debug!(
            "Client::connect -> spawning raw packet with id: {:#?}",
            raw_packet_id
        );

        let event_id = world.spawn((SendPacket {
            packet_id: raw_packet_id,
        },));
        debug!(
            "Client::connect -> spawning event `SendPacket` with id: {:#?}",
            event_id
        );
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
        self.prepare_packet_to_send();
        self.sender();
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

        for (event_id, (event,)) in world.query::<(&ReceivedNewPacket,)>().iter() {
            debug!(
                "Client::on_received_new_packet received new packet event {:#?}",
                event_id
            );

            let packet_id = event.packet_id;

            let mut packet_query = world
                .query_one::<(&Header, &Footer, &Address)>(packet_id)
                .unwrap();
            let (header, footer, address) = packet_query.get().unwrap();
            debug!(
                "Client::on_received_new_packet packet {:#?} info {:#?} {:#?} {:#?}",
                packet_id, header, footer, address
            );

            let connection_id = footer.connection_id;

            // NOTE(alex): Check if this packet has a `Source`.
            if let Some(host_id) = world
                .query::<(&Address,)>() // both host and packet archetypes have this
                .without::<Header>() // only packets have a `Header` component
                .iter()
                .find_map(|(host_id, (host_address,))| {
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
                // out. TODO(alex) 2021-04-22: The `new_sequence > old_sequence`
                // check avoids the case where a packet arrives out of order, and
                // should not be marked as the latest, but this won't
                // hold if sequence wraps.
                let old_latest_id = world
                    .query::<(&Header, &Source)>()
                    .with::<LatestReceived>()
                    .iter()
                    .find_map(|(packet_id, (old_header, source))| {
                        (source.host_id == host_id && header.sequence > old_header.sequence)
                            .then_some(packet_id)
                    });

                known_host_packets.push((
                    packet_id,
                    host_id,
                    old_latest_id,
                    header.clone(),
                    connection_id,
                ));
            } else {
                debug!("Client::on_received_new_packet packet is invalid");
                // NOTE(alex): `Source`less packets can only be connection requests.
                invalid_packets.push(packet_id);
            }

            handled_events.push(event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
            debug!(
                "Client::on_received_new_packet despawning handled event {:#?}",
                event_id
            );
        }

        while let Some((packet_id, host_id, old_latest_id, header, connection_id)) =
            known_host_packets.pop()
        {
            // WARNING(alex): rust and rust-analyzer won't give an error if you forget to import the
            // constants that are to be `match`ed, instead it will do the normal destructuring
            // behaviour, and short-circuit whatever comes after the first non-imported name!
            // rust-analyzer will at least squiggle it suggesting that you use lower-case instead of
            // all-caps, as it thinks you're just creating a binding.
            let packet_type_flags = (header.status_code >> 4) & 0b1111_1111_1111;
            match (packet_type_flags, connection_id) {
                (CONNECTION_DENIED, None) => {
                    let event_id = world.spawn((ReceivedConnectionDenied { packet_id, host_id },));
                    debug!(
                        "Client::on_received_new_packet spawning connection denied event {:#?}",
                        event_id
                    );
                }
                (CONNECTION_ACCEPTED, Some(_)) => {
                    let event_id =
                        world.spawn((ReceivedConnectionAccepted { packet_id, host_id },));
                    debug!(
                        "Client::on_received_new_packet spawning connection accepted event {:#?}",
                        event_id
                    );
                }
                (DATA_TRANSFER, Some(_)) => {
                    let event_id = world.spawn((ReceivedDataTransfer { packet_id, host_id },));
                    debug!(
                        "Client::on_received_new_packet spawning data transfer event {:#?}",
                        event_id
                    );
                }
                (HEARTBEAT, Some(_)) => {
                    let event_id = world.spawn((ReceivedHeartbeat { packet_id, host_id },));
                    debug!(
                        "Client::on_received_new_packet spawning heartbeat event {:#?}",
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

            let _ = world
                .insert(packet_id, (Source { host_id }, LatestReceived))
                .unwrap();

            if let Some(packet_id) = old_latest_id {
                let _ = world.remove::<(LatestReceived,)>(packet_id).unwrap();
                debug!(
                    "Client::on_received_new_packet swapping latest received modifier {:#?}",
                    packet_id
                );
            }

            if header.ack != 0 {
                let event_id = world.spawn((AckLocalPacket { packet_id, host_id },));
                debug!(
                    "Client::on_received_new_packet spawning ack local packet event {:#?}",
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
            let event_id = world.spawn((AckRemotePacket { packet_id, host_id },));
            debug!(
                "Client::on_received_new_packet spawning ack remote packet event {:#?}",
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

        for (event_id, event) in world.query::<&ReceivedConnectionAccepted>().iter() {
            debug!(
                "Client::on_received_connection_accepted event {:#?}",
                event_id
            );

            let mut packet_query = world
                .query_one::<(&Header, &Footer, &Source)>(event.packet_id)
                .unwrap();
            let (header, footer, source) = packet_query.get().unwrap();
            debug_assert_eq!(source.host_id, event.host_id);
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

        while let Some((host_id, packet_id, connection_id)) = connected_hosts.pop() {
            world.remove::<(AwaitingConnectionAck,)>(host_id).unwrap();
            world
                .insert(
                    host_id,
                    (Connected {
                        connection_id,
                        rtt: Duration::default(),
                    },),
                )
                .unwrap();

            // TODO(alex): Mark this packet as handled, somehow.
            // world.insert(packet_id, (Internal { time: timer.elapsed(),},),).unwrap();
        }

        while let Some(packet_id) = invalid_packets.pop() {
            world.despawn(packet_id).unwrap();
        }
    }

    pub(crate) fn on_received_connection_denied(&mut self) {
        let (buffer, world, timer) = (&mut self.buffer, &mut self.world, &self.timer);

        let mut handled_events = Vec::with_capacity(8);

        let mut denied_hosts = Vec::with_capacity(8);
        let mut invalid_packets = Vec::with_capacity(8);

        for (event_id, event) in world.query::<&ReceivedConnectionDenied>().iter() {
            let mut packet_query = world
                .query_one::<(&Header, &Source)>(event.packet_id)
                .unwrap();
            let (header, source) = packet_query.get().unwrap();
            debug_assert_eq!(source.host_id, event.host_id);

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

        while let Some((host_id, packet_id)) = denied_hosts.pop() {
            let _ = world.remove::<(AwaitingConnectionAck,)>(host_id).unwrap();
            let _ = world.insert(host_id, (Disconnected,)).unwrap();
            // TODO(alex): Mark this packet as handled, somehow.
        }

        while let Some(packet_id) = invalid_packets.pop() {
            let _ = world.despawn(packet_id).unwrap();
        }
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
