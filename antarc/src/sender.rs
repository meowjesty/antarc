use std::{
    net::{SocketAddr, UdpSocket},
    time::Instant,
};

use hecs::{Entity, Ref, World};

use crate::{
    host::{
        AckingConnection, Address, AwaitingConnectionAck, Connected, Disconnected,
        RequestingConnection,
    },
    net::NetworkResource,
    packet::{
        header::Header, Acked, ConnectionAccepted, ConnectionDenied, ConnectionRequest,
        DataTransfer, Footer, Heartbeat, Packet, Payload, Queued, Received, Sent, Sequence,
        CONNECTION_REQUEST,
    },
    readiness::Writable,
    receiver::{AckRemotePacket, LatestReceived, SendPacket, Source},
};

#[derive(Debug, PartialEq, PartialOrd)]
pub(crate) struct Destination {
    pub(crate) host_id: Entity,
}

#[derive(Debug, PartialEq, PartialOrd)]
pub(crate) struct PreparePacketToSend {
    pub(crate) packet_id: Entity,
}

#[derive(Debug, PartialEq, PartialOrd)]
pub(crate) struct RawPacket {
    pub(crate) packet_id: Entity,
    pub(crate) bytes: Vec<u8>,
    pub(crate) address: SocketAddr,
}

#[derive(Debug, PartialEq, PartialOrd)]
pub(crate) struct LatestSent;

pub(crate) fn prepare_packet_to_send(world: &mut World) {
    for (event_id, (event,)) in world.query::<(&PreparePacketToSend,)>().iter() {}
}

pub(crate) fn system_sender(
    status_code: u16,
    socket: &UdpSocket,
    timer: &Instant,
    world: &mut World,
) {
    let mut handled_events = Vec::with_capacity(8);
    let mut sent_packets = Vec::with_capacity(8);
    let mut new_footer = None;
    let mut sent = None;
    let mut old_latest_sent = None;

    if let Some((event_id, _writable)) = world.query::<&Writable>().iter().next() {
        if let Some((_, resource)) = world.query::<&mut NetworkResource>().iter().next() {
            let socket = &mut resource.socket;

            // TODO(alex) 2021-04-24: We get here after the packet is pre-processed and is ready to
            // be sent, no reason to encode it here! There is a previous event to this one, the
            // `PreparePacketToSend`, which encodes the packet and creates a `RawPacket`.
            if let Some((send_event_id, (event,))) = world.query::<(&SendPacket,)>().iter().next() {
                let mut raw_packet_query =
                    world.query_one::<(&RawPacket,)>(event.packet_id).unwrap();
                let (raw_packet,) = raw_packet_query.get().unwrap();

                match socket.send_to(&raw_packet.bytes, raw_packet.address) {
                    Ok(num_sent) => {
                        debug_assert!(num_sent > 0);

                        sent_packets.push(raw_packet.packet_id);
                        handled_events.push((event_id, send_event_id));
                    }
                    Err(fail) => {
                        eprintln!("Failed to send {:#?} with {:#?}.", raw_packet, fail);
                    }
                }
            }
        }
    }

    if let Some(packet) = world
        .query::<(&Payload, &Destination, &Address)>()
        .with::<Queued>()
        .without::<Sent>()
        .iter()
        .next()
    {
        let (packet_id, (payload, destination, address)) = packet;

        let latest_sent_to_remote = world
            .query::<(&Header, &Destination)>()
            .with::<Sent>()
            .with::<LatestSent>()
            .iter()
            .find_map(|(packet_id, (header, sent_to))| {
                (sent_to == destination).then_some((packet_id, header.clone()))
            });

        // TODO(alex) 2021-04-20: It might be possible to keep an `Option<PacketId>` reference in
        // the `Host`, and it just gets updated on every successful send, instead of querying like
        // this.
        let sequence = latest_sent_to_remote
            .as_ref()
            .map_or(unsafe { Sequence::new_unchecked(1) }, |(_, header)| {
                header.sequence
            });

        // TODO(alex) 2021-04-11: This will ignore some of the packets received, while the socket
        // isn't ready to send, other packets may arrive and take the `LatestReceived` token,
        // leaving some received packets in an ack limbo.
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
        //
        // ADD(alex) 2021-04-20: The problem above is solved, but we have a new one now, we could
        // end up lagging behind in acks. If many packets are arriving, and we can't reply acking
        // each one as they come, the following logic will be acking the packets in whatever
        // ordering their events are stored in, if this order is crescent, then `past_acks` would be
        // irrelevant, and ack lag occurs, if the order is random it might be even worse.
        //
        // ADD(alex) 2021-04-20: There must be a check to grab the `LatestReceived` and ack based on
        // it, and drop the lagged events. It's doing the `LatestReceived` bit, but we're **not**
        // dropping the lagged events.
        let (event_id, remote_to_ack) = world
            .query::<(&AckRemotePacket,)>()
            .iter()
            .find_map(|(event_id, (event,))| {
                (event.host_id == destination.host_id).then_some((event_id, packet_id))
            })
            .map_or((None, 0), |(event_id, packet_id)| {
                (
                    Some(event_id),
                    world
                        .query_one::<&Header>(packet_id)
                        .unwrap()
                        .with::<LatestReceived>()
                        .get()
                        .unwrap()
                        .sequence
                        .get(),
                )
            });

        let ack_diff = {
            let latest_ack = latest_sent_to_remote
                .as_ref()
                .map_or(0, |(_, header)| header.ack);
            let diff = remote_to_ack - latest_ack;
            diff
        };

        let past_acks = {
            let latest_past_acks = latest_sent_to_remote
                .as_ref()
                .map_or(0, |(_, header)| header.past_acks);

            (latest_past_acks << ack_diff) | 0b1
        };

        let header = Header {
            sequence,
            ack: remote_to_ack,
            past_acks,
            status_code,
            // TODO(alex) 2021-04-10: Do not allow packets with large payloads, this will be
            // properly handled with fragmentation, but right now we could just check before
            // assignment!
            payload_length: payload.len() as u16,
        };

        let connection_id = world
            .query_one::<&AckingConnection>(destination.host_id)
            .map_or(None, |mut query| query.get().map(|host| host.connection_id))
            .or_else(|| {
                world
                    .query_one::<&Connected>(destination.host_id)
                    .map_or(None, |mut query| query.get().map(|host| host.connection_id))
            });

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
                old_latest_sent = latest_sent_to_remote;
                // handled_events.push(event_id);
            }
            Err(fail) => {
                eprintln!("Failed to send packet with {:#?}.", fail);
                todo!()
            }
        }
    }

    // while let Some(event_id) = handled_events.pop().flatten() {
    //     let _ = world.despawn(event_id).unwrap();
    // }

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
