use std::{
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
    time::{Duration, Instant},
};

use hecs::World;
use log::debug;

use crate::{
    events::{
        AckLocalPacketEvent, AckRemotePacketEvent, PreparePacketToSendEvent,
        ReceivedConnectionAcceptedEvent, ReceivedConnectionDeniedEvent,
        ReceivedConnectionRequestEvent, ReceivedDataTransferEvent, ReceivedHeartbeatEvent,
        ReceivedNewPacketEvent, SendPacketEvent,
    },
    host::{Address, Disconnected, RequestingConnection},
    net::NetManager,
    packet::{
        header::Header, ConnectionId, Footer, Payload, CONNECTION_ACCEPTED, CONNECTION_DENIED,
        CONNECTION_REQUEST, DATA_TRANSFER, HEARTBEAT,
    },
    receiver::{LatestReceived, Source},
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
        self.check_readiness();
        self.receiver();
        self.on_received_new_packet();
        self.on_received_connection_request();
        self.accept_connections();
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
                .query_one::<(&Header, &Footer, &Address)>(packet_id)
                .unwrap();
            let (header, footer, address) = packet_query.get().unwrap();
            debug!(
                "Server::on_received_new_packet packet {:#?} info {:#?} {:#?} {:#?}",
                packet_id, header, footer, address
            );

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
                    "Server::on_received_new_packet packet belongs to a known host {:#?} ",
                    host_id
                );
                // NOTE(alex): Get the current `LatestReceived` packet for this host, to swap it
                // out.

                // TODO(alex) 2021-04-22: The `new_sequence > old_sequence` check avoids the case
                // where a packet arrives out of order, and should not be marked as the latest, but
                // this won't hold if sequence wraps.
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
                    footer.connection_id,
                ));
            } else if header.status_code == CONNECTION_REQUEST && footer.connection_id.is_none() {
                debug!("Server::on_received_new_packet packet belongs to an unknown host ");
                // NOTE(alex): `Source`less packet is a connection request, this is ok.
                debug_assert_eq!(header.status_code, CONNECTION_REQUEST);
                unknown_host_packets.push(packet_id);
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

        while let Some((packet_id, host_id, old_latest_id, header, connection_id)) =
            known_host_packets.pop()
        {
            // WARNING(alex): rust and rust-analyzer won't give an error if you forget to import the
            // constants that are to be `match`ed, instead it will do the normal destructuring
            // behaviour, and short-circuit whatever comes after the first non-imported name!
            // rust-analyzer will at least squiggle it suggesting that you use lower-case instead of
            // all-caps, as it thinks you're just creating a binding.
            match (header.status_code, connection_id) {
                (CONNECTION_REQUEST, None) => {
                    let event_id = world.spawn((ReceivedConnectionRequestEvent { packet_id },));
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

            let _ = world
                .insert(packet_id, (Source { host_id }, LatestReceived))
                .unwrap();

            if let Some(packet_id) = old_latest_id {
                let _ = world.remove::<(LatestReceived,)>(packet_id).unwrap();
                debug!(
                    "Server::on_received_new_packet swapping LatestReceived {:#?}",
                    packet_id
                );
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

        // NOTE(alex) 2021-04-21: As the packets here are `Source`less, this won't conflict with the
        // spawning of connection requests of `Source`d packets above, this check is done in the
        // event handling iteration.
        // NOTE(alex) 2021-04-23: These packets have been checked to be `ConnectionRequest`s.
        let events = world
            .spawn_batch(unknown_host_packets.iter().map(|packet_id| {
                (ReceivedConnectionRequestEvent {
                    packet_id: *packet_id,
                },)
            }))
            .collect::<Vec<_>>();
        if !events.is_empty() {
            debug!(
                "Server::on_received_new_packet spawning ReceivedConnectionRequest (len) {:#?}",
                events.len()
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
        let mut new_hosts = Vec::with_capacity(8);
        let mut reconnecting_hosts = Vec::with_capacity(8);
        let mut invalid_packets = Vec::with_capacity(8);

        for (event_id, event) in world.query::<&ReceivedConnectionRequestEvent>().iter() {
            debug!(
                "Server::on_received_connection_request handle ReceivedConnectionRequest {:#?}",
                event_id
            );

            let mut packet_query = world
                .query_one::<&Address>(event.packet_id)
                .unwrap()
                .with::<Header>();
            let address = packet_query.get().unwrap();
            debug!(
                "Server::on_received_connection_request packet {:#?} info {:#?}",
                event.packet_id, address
            );

            // NOTE(alex): This packet is a connection request from a known host (must be in a
            // `Disconnected` state).
            if let Some(source) = world.query_one::<&Source>(event.packet_id).unwrap().get() {
                debug!(
                    "Server::on_received_connection_request packet has a source {:#?}",
                    source
                );

                if let Some(_disconnected) = world
                    .query_one::<&Disconnected>(source.host_id)
                    .unwrap()
                    .get()
                {
                    reconnecting_hosts.push((event.packet_id, source.host_id));
                    debug!("Server::on_received_connection_request host is disconnected");
                } else {
                    // NOTE(alex): Host is in an incompatible state to receive this kind of
                    // packet.
                    invalid_packets.push(event.packet_id);
                    debug!("Server::on_received_connection_request host is in invalid state");
                }
            } else {
                // NOTE(alex): Create a new host and add it as the source for this packet.
                new_hosts.push((event.packet_id, address.clone()));
                debug!("Server::on_received_connection_request packet has no source");
            }

            handled_events.push(event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
            debug!(
                "Server::on_received_connection_request despawning ReceivedConnectionRequest {:#?}",
                event_id
            );
        }

        while let Some(packet_id) = invalid_packets.pop() {
            let _ = world.despawn(packet_id).unwrap();
            debug!(
                "Server::on_received_connection_request despawning invalid packet {:#?}",
                packet_id
            );
        }

        while let Some((packet_id, host_id)) = reconnecting_hosts.pop() {
            let _ = world.remove::<(Disconnected,)>(host_id).unwrap();
            let _ = world
                .insert(host_id, (RequestingConnection { attempts: 0 },))
                .unwrap();
            debug!(
                "Server::on_received_connection_request Disconnected -> RequestingConnection {:#?}",
                host_id
            );
        }

        // TODO(alex) 2021-04-22: This and every other case of `spawn` could be changed to
        // `spawn_batch` something like: `spawn_batch(list.iter()).for_each(|id|
        // world.insert(id, component))`.
        while let Some((packet_id, address)) = new_hosts.pop() {
            let host_id = world.spawn((address, RequestingConnection { attempts: 0 }));
            let _ = world
                .insert(packet_id, (LatestReceived, Source { host_id }))
                .unwrap();

            let _ = world.spawn((AckRemotePacketEvent {
                packet_id,
                destination_id: host_id,
            },));
            debug!(
                "Server::on_received_connection_request spawning Source {:#?}",
                host_id
            );
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

    pub(crate) fn accept_connections(&mut self) {
        let world = &mut self.world;
        let mut awaiting_connection_ack = Vec::with_capacity(8);

        for (host_id, (address, requesting_connection)) in
            world.query::<(&Address, &RequestingConnection)>().iter()
        {
            debug_assert!({
                world
                    .query::<(&Header, &LatestReceived, &Source, &Address)>()
                    .iter()
                    .any(|(_, (header, _, source, packet_address))| {
                        source.host_id == host_id
                            && header.status_code == CONNECTION_REQUEST
                            && packet_address == address
                    })
            });

            awaiting_connection_ack.push((host_id, address.clone()));
        }

        while let Some((host_id, address)) = awaiting_connection_ack.pop() {
            let _event_id = world.spawn((PreparePacketToSendEvent {
                destination_id: host_id,
                payload: Payload(Vec::new()),
                status_code: CONNECTION_ACCEPTED,
                address,
            },));
        }
    }

    pub fn retrieve(&self) -> Vec<(u32, Vec<u8>)> {
        todo!();
    }

    pub fn enqueue(&self, data: Vec<u8>) {
        todo!()
    }

    pub fn ban_host(&self, host_id: u32) {
        todo!();
    }
}
