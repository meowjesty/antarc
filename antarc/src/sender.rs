use std::{net::UdpSocket, time::Instant};

use hecs::{Entity, Ref, World};

use crate::{
    host::{
        AckingConnection, Address, AwaitingConnectionAck, Connected, Disconnected, HasConnectionId,
        Host, RequestingConnection,
    },
    packet::{
        header::Header, Acked, ConnectionAccepted, ConnectionDenied, ConnectionRequest,
        DataTransfer, Footer, Heartbeat, Packet, Payload, Received, Sent, Sequence, ToSend,
        CONNECTION_REQUEST,
    },
};

#[derive(Debug)]
pub(crate) struct Destination {
    pub(crate) host_id: Entity,
}

pub(crate) fn system_sender(socket: &UdpSocket, timer: &Instant, world: &mut World) {
    let mut new_footer = None;
    let mut sent = None;

    if let Some(packet) = world
        .query::<(&Header, &Payload, &Destination, &Address)>()
        .with::<ToSend>()
        .without::<Sent>()
        .without::<Acked>()
        .iter()
        .next()
    {
        let (packet_id, (header, payload, destination, address)) = packet;

        let connection_id = world
            .get::<HasConnectionId>(destination.host_id)
            .map(|has_connection_id| has_connection_id.get())
            .ok();

        // TODO(alex) 2021-04-04: This is going to send only 1 packet, we're using `next` on the
        // iterator and not looping! This system is part of a `tick` that has to be constantly
        // called, the looping shouldn't be done here, I think.
        //
        // TODO(alex) 2021-04-05: `new_footer` will always exist here, but when we start handling
        // encoding errors, then it might not be created.
        let (packet_bytes, footer) = Packet::encode(header, payload, connection_id).unwrap();
        new_footer = Some((packet_id, (footer,)));

        match socket.send_to(&packet_bytes, address.0) {
            Ok(num_sent) => {
                debug_assert!(num_sent > 0);
                sent = Some((packet_id, destination.host_id));
            }
            Err(fail) => {
                eprintln!("Failed to send packet with {:#?}.", fail);
                todo!()
            }
        }
    }

    if let Some((packet_id, (footer,))) = new_footer {
        world.insert(packet_id, (footer,)).unwrap();
    }

    if let Some((packet_id, host_id)) = sent {
        world
            .insert(
                packet_id,
                (Sent {
                    time: timer.elapsed(),
                },),
            )
            .unwrap();

        let mut host = world.get_mut::<Host>(host_id).unwrap();
        host.sequence_tracker = Sequence::new(host.sequence_tracker.get() + 1).unwrap();
    }
}

// TODO(alex) 2021-04-05: We could use a `NeedsUpdate` or some sort of `Changed` indicator for a
// `Host` entity (and even the `Packet`, maybe) to get to whatever host should run in the next
// system, asap. Maybe this could be something as simple as not having another system, and just
// doing what is needed next.
//
// What do we need to do now?
// - Ack + past_ack tracking is updated on the **receiver** side;
// - Calculating rtt is also done on the receiver side (on packet received);
// - `system_sender` doesn't care what kind of packet this is, only that it's `ToSend`;
//
// Calculate rtt, ack packet, calculate past_acks.
