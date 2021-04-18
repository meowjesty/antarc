use std::{net::UdpSocket, time::Instant};

use hecs::{Entity, Ref, World};

use crate::{
    host::{
        AckingConnection, Address, AwaitingConnectionAck, Connected, Disconnected,
        RequestingConnection,
    },
    packet::{
        header::Header, Acked, ConnectionAccepted, ConnectionDenied, ConnectionRequest,
        DataTransfer, Footer, Heartbeat, Packet, Payload, Received, Sent, Sequence, ToSend,
        CONNECTION_REQUEST,
    },
    receiver::{LatestReceived, Source},
};

#[derive(Debug, PartialEq, PartialOrd)]
pub(crate) struct Destination {
    pub(crate) host_id: Entity,
}

#[derive(Debug, PartialEq, PartialOrd)]
pub(crate) struct LatestSent;

pub(crate) fn system_sender(
    status_code: u16,
    socket: &UdpSocket,
    timer: &Instant,
    world: &mut World,
) {
    let mut new_footer = None;
    let mut sent = None;
    let mut old_latest_sent = None;

    if let Some(packet) = world
        .query::<(&Payload, &Destination, &Address)>()
        .with::<ToSend>()
        .without::<Sent>()
        .without::<Acked>()
        .iter()
        .next()
    {
        let (packet_id, (payload, destination, address)) = packet;

        let latest_sent = world
            .query::<(&Header, &Destination)>()
            .with::<Sent>()
            .with::<LatestSent>()
            .iter()
            .find_map(|(packet_id, (header, sent_to))| {
                (sent_to == destination).then_some((packet_id, header.clone()))
            });

        let sequence = latest_sent
            .as_ref()
            .map_or(unsafe { Sequence::new_unchecked(1) }, |(_, header)| {
                header.sequence
            });

        // TODO(alex) 2021-04-11: This will ignore some of the packets received, while the socket
        // isn't ready to send, other packets may arrive and take the `LatestReceived` token,
        // leaving a some received packets in an ack limbo.
        //
        // To counter this, some `ToAck(packet_id)` archetype would be nice, as we could loop
        // through those here, grab the biggest ack number, take the difference between whatever
        // packet never arrived, and use this to create the true `past_acks` value, example:
        //
        // sent { sequence 20, ack 19, past_acks 1111_0111 }
        // received { sequence 20 }
        // received { sequence 21 }
        // lost received { sequence 22 }
        // lost received { sequence 23 }
        // received { sequence 24 }
        // sent { sequence 21, ack 24, past_acks 0111_1100 }
        //
        // To achieve this, something must be done in the receiver side of things, more than just
        // strapping a `LatestReceived` marker. We need a list of packets to ack.
        let ack = world
            .query::<(&Header, &Source)>()
            .with::<LatestReceived>()
            .iter()
            .find_map(|(_, (header, source))| {
                (source.host_id == destination.host_id).then_some(header.sequence)
            })
            .map_or(0, |sequence_to_ack| sequence_to_ack.get());

        let ack_diff = {
            let latest_ack = latest_sent.as_ref().map_or(0, |(_, header)| header.ack);
            let diff = ack - latest_ack;
            diff
        };

        let past_acks = {
            let latest_past_acks = latest_sent
                .as_ref()
                .map_or(0, |(_, header)| header.past_acks);

            (latest_past_acks << ack_diff) | 0b1
        };

        let header = Header {
            sequence,
            ack,
            past_acks,
            status_code,
            // TODO(alex) 2021-04-10: Do not allow packets with large payloads, this will be
            // properly handled with fragmentation, but right now we could just check before
            // assignment!
            payload_length: payload.len() as u16,
        };

        todo!();
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
        let (packet_bytes, footer) = Packet::encode(&header, payload, connection_id).unwrap();
        new_footer = Some((packet_id, (footer,)));

        match socket.send_to(&packet_bytes, address.0) {
            Ok(num_sent) => {
                debug_assert!(num_sent > 0);
                sent = Some((packet_id, destination.host_id));
                old_latest_sent = latest_sent;
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

        // let mut host = world.get_mut::<Host>(host_id).unwrap();
        // host.sequence_tracker = Sequence::new(host.sequence_tracker.get() + 1).unwrap();
    }

    if let Some((packet_id, _)) = old_latest_sent {
        let _ = world.remove::<(LatestSent,)>(packet_id).unwrap();
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
