use std::{net::UdpSocket, time::Instant};

use hecs::World;

use crate::{
    host::Address,
    packet::{Packet, Received},
};

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
