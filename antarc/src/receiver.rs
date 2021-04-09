use std::{net::UdpSocket, time::Instant};

use hecs::{Entity, Ref, World};

use crate::{
    host::{
        Address, AwaitingConnectionAck, Connected, Disconnected, HasConnectionId, Host,
        RequestingConnection,
    },
    packet::{
        header::Header, ConnectionAccepted, ConnectionDenied, ConnectionId, ConnectionRequest,
        DataTransfer, Footer, Heartbeat, Internal, Packet, Payload, Received, Retrieved,
        CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST, DATA_TRANSFER, HEARTBEAT,
    },
};

#[derive(Debug)]
pub(crate) struct Source {
    pub(crate) host_id: Entity,
}

#[derive(Debug)]
pub(crate) struct OnReceived;

pub(crate) fn system_receiver(
    socket: &UdpSocket,
    buffer: &mut [u8],
    timer: &Instant,
    world: &mut World,
) {
    match socket.recv_from(buffer) {
        Ok((num_recv, from_addr)) => {
            if num_recv > 0 {
                let (header, payload, footer) = Packet::decode(&buffer).unwrap();
                let packet = world.spawn((
                    payload,
                    Address(from_addr),
                    Received {
                        time: timer.elapsed(),
                    },
                    OnReceived,
                ));

                let packet_type_flags = (header.status_code >> 4) & 0b1111_1111_1111;

                // WARNING(alex): rust and rust-analyzer won't give an error if you forget to import
                // the constants that are to be `match`ed, instead it will do the normal
                // destructuring behaviour, and short-circuit whatever comes after the first
                // non-imported name! rust-analyzer will at least squiggle it suggesting that you
                // use lower-case instead of all-caps, as it thinks you're just creating a binding.
                match (packet_type_flags, footer.connection_id) {
                    (CONNECTION_REQUEST, None) => {
                        world.insert(packet, (ConnectionRequest,)).unwrap();
                    }
                    (CONNECTION_DENIED, None) => {
                        world.insert(packet, (ConnectionDenied,)).unwrap();
                    }
                    (CONNECTION_ACCEPTED, Some(_)) => {
                        world.insert(packet, (ConnectionAccepted,)).unwrap();
                    }
                    (DATA_TRANSFER, Some(_)) => {
                        world.insert(packet, (DataTransfer,)).unwrap();
                    }
                    (HEARTBEAT, Some(_)) => {
                        world.insert(packet, (Heartbeat,)).unwrap();
                    }
                    invalid => {
                        eprintln!("Invalid packet type received {:#?}.", invalid);
                        world.despawn(packet).unwrap();
                        unreachable!();
                    }
                };

                world.insert(packet, (header, footer)).unwrap();
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
}

pub(crate) fn system_on_packet_received(world: &mut World) {
    let mut connection_request_packets = Vec::with_capacity(8);
    let mut host_received_list = Vec::with_capacity(8);
    let mut invalid_packets = Vec::with_capacity(8);
    // let mut new_host_list = Vec::with_capacity(8);

    for (packet_id, (header, packet_address)) in world
        .query::<(&Header, &Address)>()
        .with::<Footer>()
        .with::<Received>()
        .with::<OnReceived>()
        .without::<Source>()
        .iter()
    {
        let mut connection_request_query =
            world.query_one::<&ConnectionRequest>(packet_id).unwrap();

        // NOTE(alex): Check if this packet has a `Source`.
        if let Some(host_id) = world.query::<(&Address,)>().with::<Host>().iter().find_map(
            |(host_id, (host_address,))| {
                if host_address == packet_address {
                    Some(host_id)
                } else {
                    None
                }
            },
        ) {
            host_received_list.push((host_id, packet_id))
        } else if let Some(_) = connection_request_query.get() {
            // NOTE(alex): `Source`less packet is a connection request, this is ok.
            connection_request_packets.push(packet_id);
        } else {
            // NOTE(alex): `Source`less packets can only be connection requests.
            invalid_packets.push(packet_id);
        }
    }

    while let Some((host_id, packet_id)) = host_received_list.pop() {
        world.insert(packet_id, (Source { host_id },)).unwrap();
    }

    while let Some(packet_id) = connection_request_packets.pop() {
        world.remove::<(OnReceived,)>(packet_id).unwrap();
    }

    while let Some(packet_id) = invalid_packets.pop() {
        let _ = world.despawn(packet_id).unwrap();
    }
}

fn system_received_connection_request(world: &mut World) {
    let mut new_hosts = Vec::with_capacity(8);
    let mut reconnecting_hosts = Vec::with_capacity(8);
    let mut invalid_packets = Vec::with_capacity(8);

    for (packet_id, (header, address)) in world
        .query::<(&Header, &Address)>()
        .with::<Footer>()
        .with::<Received>()
        .with::<ConnectionRequest>()
        // NOTE(alex): This is the only place where you need to exclude `OnReceived` marker, as
        // we're not sure if the packet contains a `Source` or not yet, every other system will deal
        // with a `Source` component.
        .without::<OnReceived>()
        .iter()
    {
        // NOTE(alex): There is a known host, but is it in the correct state to handle a connection
        // request packet?
        if let Ok(source) = world.get::<Source>(packet_id) {
            let host_query = world.query_one::<&Host>(source.host_id).unwrap();

            if let Some(_) = host_query.with::<Disconnected>().get() {
                // NOTE(alex): `Disconnected -> RequestingConnection` is possible.
                reconnecting_hosts.push(source.host_id);
            } else {
                // NOTE(alex): `* -> RequestingConnection` would be an invalid state transition.
                invalid_packets.push(packet_id);
            }
        } else {
            let host = Host {
                ack_tracker: header.sequence.get(),
                ..Default::default()
            };

            new_hosts.push((packet_id, (host, address.clone())));
        }
    }

    // NOTE(alex): Handle packets from new hosts requesting connection.
    while let Some((packet_id, (new_host, address))) = new_hosts.pop() {
        let host_id = world.spawn((new_host, address, RequestingConnection { attempts: 0 }));
        world.insert(packet_id, (Source { host_id },)).unwrap();
    }

    // NOTE(alex): Handle packets from hosts that were disconnected, and are trying to connect.
    while let Some(host_id) = reconnecting_hosts.pop() {
        let _ = world.remove::<(Disconnected,)>(host_id).unwrap();
        let _ = world
            .insert(host_id, (RequestingConnection { attempts: 0 },))
            .unwrap();
    }

    while let Some(packet_id) = invalid_packets.pop() {
        let _ = world.despawn(packet_id).unwrap();
    }
}

fn system_received_connection_accepted(timer: &Instant, world: &mut World) {
    let mut connection_accepted = Vec::with_capacity(8);
    let mut invalid_packets = Vec::with_capacity(8);

    for (packet_id, (footer, source)) in world
        .query::<(&Footer, &Source)>()
        .with::<Header>()
        .with::<Received>()
        .with::<Address>()
        .with::<ConnectionAccepted>()
        .without::<Internal>()
        .iter()
    {
        match world
            .query::<(&AwaitingConnectionAck,)>()
            .with::<Host>()
            .iter()
            .find_map(|(host_id, (_,))| {
                if host_id == source.host_id {
                    Some(host_id)
                } else {
                    None
                }
            }) {
            // NOTE(alex): We have a `Host` in a valid state that can take this type of packet.
            Some(host_id) => {
                debug_assert!(footer.connection_id.is_some());
                connection_accepted.push((host_id, (footer.connection_id.unwrap(),), packet_id));
            }
            // NOTE(alex): No `Host` is awaiting for this type of packet.
            None => {
                eprintln!("Tried to `ConnectionAccepted` packet, but no `Host` in suitable state was found!");
                invalid_packets.push(packet_id);
            }
        }
    }

    // NOTE(alex): Change `Host` state to `Connected`, and mark packet as handled.
    while let Some((host_id, (connection_id,), packet_id)) = connection_accepted.pop() {
        world.remove::<(AwaitingConnectionAck,)>(host_id).unwrap();
        world
            .insert(host_id, (Connected, HasConnectionId(connection_id)))
            .unwrap();

        world
            .insert(
                packet_id,
                (Internal {
                    time: timer.elapsed(),
                },),
            )
            .unwrap();
    }

    while let Some(packet_id) = invalid_packets.pop() {
        world.despawn(packet_id).unwrap();
    }
}

fn system_received_connection_denied(timer: &Instant, world: &mut World) {
    let mut denied_hosts = Vec::with_capacity(8);
    let mut invalid_packets = Vec::with_capacity(8);

    for (packet_id, (header, source, received)) in world
        .query::<(&Header, &Source, &Received)>()
        .with::<ConnectionDenied>()
        .without::<Internal>()
        .iter()
    {
        let host_query = world.query_one::<&Host>(source.host_id).unwrap();
        // NOTE(alex): Only hosts with `AwaitingConnectionAck` may handle this type of packet.
        if let Some(_) = host_query.with::<AwaitingConnectionAck>().get() {
            denied_hosts.push((source.host_id, packet_id));
        } else {
            invalid_packets.push(packet_id);
        }
    }

    while let Some((host_id, packet_id)) = denied_hosts.pop() {
        let _ = world.remove::<(AwaitingConnectionAck,)>(host_id).unwrap();
        // TODO(alex) 2021-04-09: Reset `Host` tracker values.
        let _ = world.insert(host_id, (Disconnected,)).unwrap();

        let _ = world
            .insert(
                packet_id,
                (Internal {
                    time: timer.elapsed(),
                },),
            )
            .unwrap();
    }

    while let Some(packet_id) = invalid_packets.pop() {
        let _ = world.despawn(packet_id).unwrap();
    }
}


// TODO(alex) 2021-04-08: `system_received_heartbeat`.

// TODO(alex) 2021-04-09: `system_on_receive_ack` that loops through packets with `Source` and acks
// the host's sent packets. Is this system ok with being run in any order (probably not, as it
// shouldn't loop through packets `Internal` and `Retrieved`, but there is no way to avoid this
// right now).
