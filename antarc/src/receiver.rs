use std::{net::UdpSocket, time::Instant};

use hecs::{Entity, Ref, World};

use crate::{
    host::{
        Address, AwaitingConnectionAck, Connected, Disconnected, HasConnectionId, Host,
        RequestingConnection,
    },
    packet::{
        header::Header, ConnectionAccepted, ConnectionDenied, ConnectionRequest, DataTransfer,
        Footer, Heartbeat, Packet, Received, CONNECTION_ACCEPTED, CONNECTION_DENIED,
        CONNECTION_REQUEST, DATA_TRANSFER, HEARTBEAT,
    },
};

#[derive(Debug)]
pub(crate) struct Source {
    pub(crate) host_id: Entity,
}

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

pub(crate) fn system_received_packet(world: &mut World) {
    let mut host_received_list = world
        .query::<(&Header, &Address)>()
        .with::<Footer>()
        .with::<Received>()
        .without::<Source>()
        .iter()
        .filter_map(|(packet_id, (_, packet_address))| {
            let host_packets = world.query::<(&Address,)>().with::<Host>().iter().find_map(
                |(host_id, (host_address,))| {
                    if host_address == packet_address {
                        Some((host_id, packet_id))
                    } else {
                        None
                    }
                },
            );
            host_packets
        })
        .collect::<Vec<_>>();

    while let Some((host_id, packet_id)) = host_received_list.pop() {
        world.insert(packet_id, (Source { host_id },)).unwrap();
    }
}

fn system_received_connection_request_from_new_host(world: &mut World) {
    let mut requesting_connection = world
        .query::<(&Header, &Address)>()
        .with::<Footer>()
        .with::<Received>()
        .with::<ConnectionRequest>()
        .without::<Source>()
        .iter()
        .map(|(packet_id, (header, address))| {
            let new_host = Host {
                ack_tracker: header.sequence.get(),
                ..Default::default()
            };

            (new_host, packet_id, address.clone())
        })
        .collect::<Vec<_>>();

    while let Some((new_host, packet_id, packet_address)) = requesting_connection.pop() {
        let host_id = world.spawn((
            new_host,
            packet_address,
            RequestingConnection { attempts: 0 },
        ));
        world.insert(packet_id, (Source { host_id },)).unwrap();
    }
}

fn system_received_connection_accepted(world: &mut World) {
    let mut connection_accepted = world
        .query::<(&Footer, &Source)>()
        .with::<Header>()
        .with::<Received>()
        .with::<Address>()
        .with::<ConnectionAccepted>()
        .iter()
        .filter_map(|(packet_id, (footer, source))| {
            world
                .query::<(&AwaitingConnectionAck,)>()
                .with::<Host>()
                .iter()
                .find_map(|(host_id, (_,))| {
                    if host_id == source.host_id {
                        Some(host_id)
                    } else {
                        None
                    }
                })
                .zip(Some((
                    packet_id,
                    footer.connection_id.unwrap(),
                    ConnectionAccepted,
                )))
        })
        .collect::<Vec<_>>();

    // TODO(alex) 2021-04-03: Is this complete?
    while let Some((host_id, (packet_id, connection_id, accepted))) = connection_accepted.pop() {
        world.remove::<(AwaitingConnectionAck,)>(host_id).unwrap();
        world
            .insert(host_id, (Connected, HasConnectionId(connection_id)))
            .unwrap();
    }
}
