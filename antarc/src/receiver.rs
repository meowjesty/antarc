// NOTE(alex) 2021-04-13: How I would like UDP sockets to work:
// - `bind(7777, buffer[10mb])` would both open the socket, and give it a buffer where data is
// loaded into, even without calling `recv`, kinda like a file read;
// - when you call `recv_from` it returns some sort of slice into the buffer, or maybe you pass it
// another buffer to put the data in, but it doesn't block anything (no wait for recv), because
// the OS already has data in the socket buffer (or no data), so recv returns `(address, data[])`;
// - every packet sent to the bound port, is just added to the socket buffer, and you tell the OS
// what should happen in case the memory can't hold up.

/// TODO(alex) 2021-04-15: Benchmarks show that spawning/despawning is faster than
/// insert/remove, this is a positive for the ecs event system. We'll be taking a small
/// performance hit that would be avoidable with a proper synchronization of systems (having
/// the `system_receiver` function call the `system_on_received_packet`, for example), but this
/// isn't a priority right now.
///
/// The big performance gains can be noticed when compared to the previous system of
/// inserting/removing markers to a packet.

/// TODO(alex) 2021-04-20: Packets may be sent twice (or more) and we're not handling this.
use std::{
    net::UdpSocket,
    time::{Duration, Instant},
};

use hecs::{Entity, Ref, World};
use log::debug;

use crate::{
    host::{Address, AwaitingConnectionAck, Connected, Disconnected, RequestingConnection},
    net::{NetManager, NetworkResource},
    packet::{
        header::Header, Acked, ConnectionAccepted, ConnectionDenied, ConnectionId,
        ConnectionRequest, DataTransfer, Footer, Heartbeat, Internal, Packet, Payload, Received,
        Retrieved, Sent, CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST, DATA_TRANSFER,
        HEARTBEAT,
    },
    readiness::Readable,
    sender::Destination,
};

#[derive(Debug, PartialEq)]
pub(crate) struct Source {
    pub(crate) host_id: Entity,
}

/// Raised when a packet is first received by the `socket`.
#[derive(Debug, PartialEq)]
pub(crate) struct ReceivedNewPacket {
    pub(crate) packet_id: Entity,
}

/// Raised when the packet `status_code` is identified as being a subset of a connection request.
/// TODO(alex) 2021-04-21: These events could hold more information than just the ids of each entity
/// to avoid the need to query for additional data on each event handler.
#[derive(Debug, PartialEq)]
pub(crate) struct ReceivedConnectionRequest {
    pub(crate) packet_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct ReceivedConnectionDenied {
    pub(crate) packet_id: Entity,
    host_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct ReceivedConnectionAccepted {
    pub(crate) packet_id: Entity,
    host_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct ReceivedDataTransfer {
    pub(crate) packet_id: Entity,
    host_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct ReceivedHeartbeat {
    pub(crate) packet_id: Entity,
    host_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct SendPacket {
    pub(crate) packet_id: Entity,
}

#[derive(Debug, PartialEq)]
enum ReceivedType {
    ConnectionRequest,
    ConnectionDenied,
    ConnectionAccepted,
    DataTransfer,
    Heartbeat,
}

/// Event: `Host` has `Sent` packets that require acking (remote acks local).
#[derive(Debug, PartialEq)]
pub(crate) struct AckLocalPacket {
    pub(crate) packet_id: Entity,
    pub(crate) host_id: Entity,
}

/// Event: `Host` has `Received` packets that will be acked on the next `send` (local acks remote).
#[derive(Debug, PartialEq)]
pub(crate) struct AckRemotePacket {
    pub(crate) packet_id: Entity,
    pub(crate) host_id: Entity,
}

#[derive(Debug, PartialEq)]
pub(crate) struct LatestReceived;

impl<T> NetManager<T> {
    ///
    /// - Raises the `ReceivedNewPacket` event.
    pub(crate) fn receiver(&mut self) {
        debug!("NetManager::receiver start");

        let (buffer, world, timer) = (&mut self.buffer, &mut self.world, &self.timer);

        let mut handled_events = Vec::with_capacity(8);
        let mut received_new_packet = None;

        if let Some((event_id, _readable)) = world.query::<&Readable>().iter().next() {
            debug!("readable event with id {:#?}", event_id);

            if let Some((resource_id, resource)) =
                world.query::<&mut NetworkResource>().iter().next()
            {
                debug!("network resource id {:#?}", resource_id);

                let socket = &mut resource.socket;

                match socket.recv_from(buffer) {
                    Ok((num_recv, from_addr)) => {
                        debug!("received a packet {:#?} with {:#?} bytes", buffer, num_recv);

                        if num_recv > 0 {
                            let remote_address = Address(from_addr);

                            let (header, payload, footer) = Packet::decode(&buffer).unwrap();

                            let received = Received {
                                time: timer.elapsed(),
                            };
                            let packet = (header, payload, footer, remote_address, received);
                            received_new_packet = Some(packet);
                        } else {
                            eprintln!("Received 0 bytes from {:#?}.", from_addr);
                            unreachable!();
                        }
                    }
                    Err(fail) => {
                        eprintln!("Failed to receive on socket with {:#?}.", fail);
                        todo!()
                    }
                }
                handled_events.push(event_id);
            }
        }

        if let Some((header, payload, footer, address, received)) = received_new_packet {
            let packet_id = world.spawn((header, payload, footer, address, received));
            debug!("spawning new packet received {:#?}", packet_id);

            let event_id = world.spawn((ReceivedNewPacket { packet_id },));
            debug!("spawning received new packet event {:#?}", event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
            debug!("despawning handled event {:#?}", event_id);
        }

        debug!("NetManager::receiver end");
    }

    /// System responsible for attributing a `Source` host to a packet entity, if the host matches
    /// the packet's `Address`, otherwise it raises the `OnReceivedConnectionRequest` event (if
    /// the `Header` represents a `ConnectionRequest`).
    ///
    /// Also handles the changes to a host's `LatestReceived` packet.
    ///
    /// - Raises the `OnReceivedConnectionRequest` event.
    pub(crate) fn on_received_new_packet(&mut self) {
        debug!("NetManager::on_received_new_packet start");

        let (buffer, world, timer) = (&mut self.buffer, &mut self.world, &self.timer);

        let mut handled_events = Vec::with_capacity(8);

        let mut unknown_host_packets = Vec::with_capacity(8);
        let mut known_host_packets = Vec::with_capacity(8);
        let mut invalid_packets = Vec::with_capacity(8);

        for (event_id, (event,)) in world.query::<(&ReceivedNewPacket,)>().iter() {
            debug!("received new packet event {:#?}", event_id);

            let packet_id = event.packet_id;

            let mut packet_query = world
                .query_one::<(&Header, &Footer, &Address)>(packet_id)
                .unwrap();
            let (header, footer, address) = packet_query.get().unwrap();
            debug!(
                "packet {:#?} info {:#?} {:#?} {:#?}",
                packet_id, header, footer, address
            );

            let packet_type_flags = (header.status_code >> 4) & 0b1111_1111_1111;
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
                debug!("packet belongs to a known host {:#?} ", host_id);
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
            } else if packet_type_flags == CONNECTION_REQUEST && connection_id.is_none() {
                debug!("packet belongs to an unknown host ");
                // NOTE(alex): `Source`less packet is a connection request, this is ok.
                debug_assert_eq!(header.status_code, CONNECTION_REQUEST);
                unknown_host_packets.push(packet_id);
            } else {
                debug!("packet is invalid");
                // NOTE(alex): `Source`less packets can only be connection requests.
                invalid_packets.push(packet_id);
            }

            handled_events.push(event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
            debug!("despawning handled event {:#?}", event_id);
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
                (CONNECTION_REQUEST, None) => {
                    let event_id = world.spawn((ReceivedConnectionRequest { packet_id },));
                    debug!("spawning received connection request event {:#?}", event_id);
                }
                (CONNECTION_DENIED, None) => {
                    let event_id = world.spawn((ReceivedConnectionDenied { packet_id, host_id },));
                    debug!("spawning received connection denied event {:#?}", event_id);
                }
                (CONNECTION_ACCEPTED, Some(_)) => {
                    let event_id =
                        world.spawn((ReceivedConnectionAccepted { packet_id, host_id },));
                    debug!(
                        "spawning received connection accepted event {:#?}",
                        event_id
                    );
                }
                (DATA_TRANSFER, Some(_)) => {
                    let event_id = world.spawn((ReceivedDataTransfer { packet_id, host_id },));
                    debug!("spawning received data transfer event {:#?}", event_id);
                }
                (HEARTBEAT, Some(_)) => {
                    let event_id = world.spawn((ReceivedHeartbeat { packet_id, host_id },));
                    debug!("spawning received heartbeat event {:#?}", event_id);
                }
                invalid => {
                    eprintln!("Invalid packet type received {:#?}.", invalid);
                    let _ = world.despawn(packet_id).unwrap();
                    unreachable!();
                }
            };

            let _ = world
                .insert(packet_id, (Source { host_id }, LatestReceived))
                .unwrap();

            if let Some(packet_id) = old_latest_id {
                let _ = world.remove::<(LatestReceived,)>(packet_id).unwrap();
                debug!("swapping latest received modifier {:#?}", packet_id);
            }

            if header.ack != 0 {
                let event_id = world.spawn((AckLocalPacket { packet_id, host_id },));
                debug!("swapping ack local packet event {:#?}", event_id);
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
            debug!("swapping ack remote packet event {:#?}", event_id);
        }

        // NOTE(alex) 2021-04-21: As the packets here are `Source`less, this won't conflict with the
        // spawning of connection requests of `Source`d packets above, this check is done in the
        // event handling iteration.
        // NOTE(alex) 2021-04-23: These packets have been checked to be `ConnectionRequest`s.
        let events = world
            .spawn_batch(unknown_host_packets.iter().map(|packet_id| {
                (ReceivedConnectionRequest {
                    packet_id: *packet_id,
                },)
            }))
            .collect::<Vec<_>>();
        debug!(
            "swapping received connection request events (len) {:#?}",
            events.len()
        );

        while let Some(packet_id) = invalid_packets.pop() {
            let _ = world.despawn(packet_id).unwrap();
            debug!("deswapping invalid packet {:#?}", packet_id);
        }

        debug!("NetManager::on_received_new_packet end");
    }

    // NOTE(alex): This handler is specific for the server, as the client doesn't receive connection
    // requests, at least not yet.
    pub(crate) fn on_received_connection_request(&mut self) {
        debug!("NetManager::on_received_connection_request start");

        let (buffer, world, timer) = (&mut self.buffer, &mut self.world, &self.timer);

        let mut handled_events = Vec::with_capacity(8);
        let mut new_hosts = Vec::with_capacity(8);
        let mut reconnecting_hosts = Vec::with_capacity(8);
        let mut invalid_packets = Vec::with_capacity(8);

        for (event_id, event) in world.query::<&ReceivedConnectionRequest>().iter() {
            debug!("received connection request event {:#?}", event_id);

            let mut packet_query = world
                .query_one::<&Address>(event.packet_id)
                .unwrap()
                .with::<Header>();
            let address = packet_query.get().unwrap();
            debug!("packet {:#?} info {:#?}", event.packet_id, address);

            // NOTE(alex): This packet is a connection request from a known host (must be in a
            // `Disconnected` state).
            if let Some(source) = world.query_one::<&Source>(event.packet_id).unwrap().get() {
                debug!("packet has a source {:#?}", source);

                if let Some(_disconnected) = world
                    .query_one::<&Disconnected>(source.host_id)
                    .unwrap()
                    .get()
                {
                    reconnecting_hosts.push((event.packet_id, source.host_id));
                    debug!("disconnected");
                } else {
                    // NOTE(alex): Host is in an incompatible state to receive this kind of
                    // packet.
                    invalid_packets.push(event.packet_id);
                    debug!("invalid");
                }
            } else {
                // NOTE(alex): Create a new host and add it as the source for this packet.
                new_hosts.push((event.packet_id, address.clone()));
                debug!("packet has no source");
            }

            handled_events.push(event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
            debug!("despawning handled event {:#?}", event_id);
        }

        while let Some(packet_id) = invalid_packets.pop() {
            let _ = world.despawn(packet_id).unwrap();
            debug!("despawning invalid packet {:#?}", packet_id);
        }

        while let Some((packet_id, host_id)) = reconnecting_hosts.pop() {
            let _ = world.remove::<(Disconnected,)>(host_id).unwrap();
            let _ = world
                .insert(host_id, (RequestingConnection { attempts: 0 },))
                .unwrap();
            debug!(
                "swapping host state to requesting connection (from disconnected) {:#?}",
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
            debug!("spawning new host as the source {:#?}", host_id);
        }
        debug!("NetManager::on_received_connection_request end");
    }

    pub(crate) fn on_received_connection_accepted(&mut self) {
        debug!("NetManager::on_received_connection_accepted start");

        let (buffer, world, timer) = (&mut self.buffer, &mut self.world, &self.timer);

        let mut handled_events = Vec::with_capacity(8);
        let mut connected_hosts = Vec::with_capacity(8);
        let mut invalid_packets = Vec::with_capacity(8);

        for (event_id, event) in world.query::<&ReceivedConnectionAccepted>().iter() {
            debug!("received connection accepted event {:#?}", event_id);

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

        debug!("NetManager::on_received_connection_accepted end");
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

    // TODO(alex) 2021-04-08: `system_received_heartbeat`.

    /// NOTE(alex): System that acks packets sent by the local host to a remote host (acks `Sent`).
    // TODO(alex): 2021-04-21: Handle this event properly, with the updated way.
    pub(crate) fn system_on_received_ack_sent_packet(&mut self) {
        let (buffer, world, timer) = (&mut self.buffer, &mut self.world, &self.timer);

        let mut handled_events = Vec::with_capacity(8);
        let mut sent_to_ack_list = Vec::with_capacity(8);

        for (event_id, (event,)) in world.query::<(&AckLocalPacket,)>().iter() {
            let AckLocalPacket { packet_id, host_id } = event;

            let mut acker_query = world.query_one::<(&Header,)>(*packet_id).unwrap();
            let (received,) = acker_query.get().unwrap();

            // TODO(alex) 2021-04-14: Handle `past_acks`. Should this be done here or raise another
            // event?
            //
            // NOTE(alex): Search for packets that have been `Sent`, but were not `Acked` yet, that
            // belong to this specific `Host` (via `Destination`) and that have a matching
            // `sequence` (sent) to this packet's `ack` (received).
            if let Some(sent) = world
                .query::<(&Header, &Sent, &Destination)>()
                .without::<Acked>()
                .iter()
                .find_map(|(sent_id, (header, sent, destination))| {
                    (destination.host_id == *host_id && header.sequence.get() == received.ack)
                        .then_some(sent_id)
                })
            {
                sent_to_ack_list.push(sent);
            }
            handled_events.push(event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
        }

        while let Some(sent_id) = sent_to_ack_list.pop() {
            // NOTE(alex): Packets here are now `Sent + Acked`.
            let _ = world
                .insert(
                    sent_id,
                    (Acked {
                        time: timer.elapsed(),
                    },),
                )
                .unwrap();
        }

        // TODO(alex) 2021-04-09: Should this add a new `ToSend` packet for a host, every packet
        // that host A receives from B should respond back with an ack, sometimes this can
        // be done via the user sending a message to B, but what if the user has no data to
        // send for a while? B might think that the packet was lost, due to the amount of
        // time between sent<->acked, this also messes up rtt calculations. But how do we
        // handle this, if we add a `ToSend` packet here, the host A sequence tracker field
        // has to be updated, as the sequence may never repeat (I don't see a problem with
        // repeating an ack value), but then if this reply fails for some reason,
        // or we ignore this packet in favor of a data transfer, then B will receive a packet with
        // the correct (possible duplicated) ack value, but it'll receive a wrong sequence
        // value (such as sequence + 2), and think that sequence + 1 was lost, when it
        // wasn't ever sent?
        //
        // ADD(alex) 2021-04-09: I'm thinking about some sort of `ToAck(ack_num)` event, instead of
        // building a full packet with a `Header`.
    }
}
