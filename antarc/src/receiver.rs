use std::{net::UdpSocket, time::Instant};

use hecs::{Entity, Ref, World};

use crate::{
    host::{Address, Disconnected, Host},
    packet::{header::Header, Packet, Received, CONNECTION_REQUEST},
};

#[derive(Debug)]
struct Source {
    host_id: Entity,
}

#[derive(Debug)]
struct Destination {
    host_id: Entity,
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
                let _packet = world.spawn((
                    header,
                    payload,
                    footer,
                    Address(from_addr),
                    Received {
                        time: timer.elapsed(),
                    },
                ));
            } else {
                eprintln!("Received 0 bytes from {:#?}.", from_addr);
                unreachable!();
            }
        }
        Err(fail) => {
            eprintln!("Failed to receive on socket with {:#?}.", fail);
            unreachable!();
        }
    }
}

pub(crate) fn system_received_packet_from_host(world: &mut World) {
    let mut host_received_list = Vec::with_capacity(12);

    // TODO(alex) 2021-03-27: Zip the two iterators by combining matching addresses somehow.
    for (packet_id, (header, packet_address)) in world
        .query::<(&Header, &Address)>()
        .with::<Received>()
        .without::<Source>()
        .iter()
    {
        for (host_id, (_,)) in world
            .query::<(&Address,)>()
            .with::<Host>()
            .iter()
            .filter(|(_, (host_address,))| *host_address == packet_address)
        {
            host_received_list.push((host_id, packet_id));
        }
    }

    while let Some((host_id, packet_id)) = host_received_list.pop() {
        world.insert(packet_id, (Source { host_id },)).unwrap();
    }

    system_received_packet_from_unknown_host(world);
}

fn system_received_packet_from_unknown_host(world: &mut World) {
    let mut new_hosts = Vec::with_capacity(12);

    for (packet_id, (header, packet_address)) in world
        .query::<(&Header, &Address)>()
        .with::<Received>()
        .without::<Source>()
        .iter()
    {
        let new_host = Host {
            ack_tracker: header.sequence.get(),
            ..Default::default()
        };

        new_hosts.push((new_host, packet_id, packet_address.clone()));
    }

    while let Some((new_host, packet_id, packet_address)) = new_hosts.pop() {
        let host_id = world.spawn((new_host, packet_address, Disconnected));
        world.insert(packet_id, (Source { host_id },)).unwrap();
    }
}

fn system_handle_received_packet(world: &mut World) {
    for (packet_id, (header, address, source)) in world
        .query::<(&Header, &Address, &Source)>()
        .with::<Received>()
        .iter()
    {
        // TODO(alex) 2021-03-28: Handle possible packet requests, move host into appropriate state.
    }
}
