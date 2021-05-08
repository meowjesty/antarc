use std::fmt;

use hecs::Entity;
use log::debug;

use crate::{
    events::{AckRemotePacketEvent, QueuedPacketEvent, SentPacketEvent},
    host::{Address, LatestSent},
    net::{NetManager, NetworkResource},
    packet::{header::Header, ConnectionId, Footer, Packet, Payload, Queued, Sent, Sequence},
    readiness::Writable,
    receiver::Source,
};

type PreparedPacket = (Header, Payload, Address, Option<ConnectionId>, Destination);

#[derive(Debug, Clone, Copy, Hash, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Destination {
    pub(crate) host_id: Entity,
}

impl From<Source> for Destination {
    fn from(source: Source) -> Self {
        Self {
            host_id: source.host_id,
        }
    }
}
impl PartialEq<Source> for Destination {
    fn eq(&self, other: &Source) -> bool {
        other.host_id == self.host_id
    }
}

#[derive(PartialEq, PartialOrd)]
pub(crate) struct RawPacket {
    pub(crate) packet_id: Entity,
    pub(crate) destination_id: Entity,
    pub(crate) bytes: Vec<u8>,
    pub(crate) address: Address,
}

impl fmt::Debug for RawPacket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawPacket")
            .field("packet_id", &self.packet_id)
            .field("bytes (partial)", &&self.bytes[0..18])
            .field("address", &self.address)
            .finish()
    }
}

impl<T> NetManager<T> {
    /*
    pub(crate) fn on_queued_packet(&mut self) {
        let world = &mut self.world;

        let mut handled_events = Vec::with_capacity(8);
        let mut queued_packets = Vec::with_capacity(8);

        // TODO(alex) 2021-04-24: Steps to do here:
        // 1. Loop through the enqueued packets event `PreparePacketToSend` and grab the
        // `Payload + Destination`;
        //
        // 2. Look for the `LatestSent` packet to this `Destination`;
        //      2.1 incrmenet sequence based on the packet sent;
        //
        // 3. Look for the `LatestReceived` packet with `Source == Destination`;
        //      3.1 ack the packet received;
        //
        // 4. Generate a `Header` based on these packets;
        //
        // 5. Only move this packet into `SendPacket` event if it's time enqueued is greater than
        // the `LatestSent` time enqueued?
        for (event_id, event) in world.query::<&QueuedPacketEvent>().iter() {
            debug!("prepare_packet_to_send -> handle PreparePacketToSend");

            // TODO(alex) 2021-04-24: This needs to loop through every packet that must be acked in
            // order to properly calculate the `past_acks`.
            //
            // ADD(alex) 2021-04-29: When do we despawn this event?
            // TODO(alex) 2021-05-05: How do we avoid a stale sequence? The sequence for this
            // specific packet is incremented here, and the tracker is only incremented after the
            // latest packet is actually sent, but the way it works right now, if the user were to
            // enqueue multiple packets, before the previous one was sent, then duplicated sequences
            // become possible.
            //
            // This applies not only to the user enqueue case, but also if the systems enqueue too
            // many packets at almost the same time.

            let (payload, status_code, address) = (
                event.payload.clone(),
                event.status_code,
                event.address.clone(),
            );

            // TODO(alex) 2021-04-29: Don't just create a connection id value here, we need to check
            // for a correct connection id, maybe it could be part of the `PreparePacketToSend`
            // event.
            let connection_id = ConnectionId::new(1);

            // TODO(alex) 2021-04-05: `new_footer` will always exist here, but when we start
            // handling encoding errors, then it might not be created.
            //
            // TODO(alex) 2021-05-07: The packet cannot be encoded here, as we don't have a sequence
            // value yet, only at the send time.
            //
            // ADD(alex) 2021-05-07: `Sequence` is not being spawned with the packet anywhere yet!
            let destination_id = event.destination_id;

            debug!(
                "prepare_packet_to_send -> data {:?} {:?}",
                address, destination_id
            );
            queued_packets.push((payload, address, status_code, connection_id, destination_id));
            handled_events.push(event_id);
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
            debug!(
                "prepare_packet_to_send -> despawning PreparePacketToSend {:?}",
                event_id
            );
        }

        while let Some((payload, address, status_code, connection_id, destination_id)) =
            queued_packets.pop()
        {
            let destination = Destination {
                host_id: destination_id,
            };
            let prepared_packet = (payload, address.clone(), destination);
            let queued_packet_id = world.spawn(prepared_packet);
            debug!(
                "prepare_packet_to_send -> spawning prepared packet {:?} ",
                queued_packet_id
            );

            let event_id = world.spawn((QueuedPacketEvent {
                queued_packet_id,
                status_code,
                connection_id,
            },));
            debug!(
                "prepare_packet_to_send -> spawning SendPacket {:?}",
                event_id
            );
        }
    }
    */

    pub(crate) fn sender(&mut self) {
        let (world, timer) = (&mut self.world, &self.timer);

        let mut handled_events = Vec::with_capacity(8);
        let mut sent_packets = Vec::with_capacity(8);

        if let Some((event_id, _writable)) = world.query::<&Writable>().iter().next() {
            debug!("sender -> handle Writable event {:#?}", event_id);

            if let Some((resource_id, resource)) =
                world.query::<&mut NetworkResource>().iter().next()
            {
                debug!("sender -> network resource id {:#?}", resource_id);

                let socket = &mut resource.socket;

                if let Some((send_event_id, (event,))) =
                    world.query::<(&QueuedPacketEvent,)>().iter().next()
                {
                    debug!("sender -> handling SendPacketEvent {:#?}", send_event_id);

                    let mut packet_query = world
                        .query_one::<(&Payload, &Address, &Destination)>(event.packet_id)
                        .unwrap();
                    let (payload, address, destination) = packet_query.get().unwrap();

                    let mut connection_query =
                        world.query_one::<&ConnectionId>(event.packet_id).unwrap();
                    let connection_id = connection_query.get().cloned();

                    let sequence = get_sequence(world, destination);
                    let ack = get_ack(world, destination);

                    let header = Header {
                        ack,
                        past_acks: 0,
                        status_code: event.status_code,
                        payload_length: payload.len() as u16,
                    };
                    debug!("sender -> header {:#?}", header);

                    let (raw_packet, footer) =
                        Packet::encode(sequence, &header, payload, connection_id).unwrap();

                    match socket.send_to(&raw_packet, address.0) {
                        Ok(num_sent) => {
                            debug!("sender -> sent a packet with {:#?} bytes", num_sent);
                            debug_assert!(num_sent > 0);

                            sent_packets.push((
                                sequence,
                                header,
                                event.packet_id,
                                destination.clone(),
                            ));
                            handled_events.push((event_id, Some(send_event_id)));
                        }
                        Err(fail) => {
                            eprintln!("Failed to send {:#?} with {:#?}.", raw_packet, fail);
                            handled_events.push((event_id, None));
                        }
                    }
                }
            }
        }

        while let Some((writable_id, send_event_id)) = handled_events.pop() {
            let _ = world.despawn(writable_id).unwrap();
            debug!(
                "sender -> despawning handled writable event {:#?}",
                writable_id
            );

            if let Some(send_event_id) = send_event_id {
                let _ = world.despawn(send_event_id).unwrap();
                debug!(
                    "sender -> despawning handled send event {:#?}",
                    send_event_id
                );
            }
        }

        while let Some((sequence, header, packet_id, destination)) = sent_packets.pop() {
            let status_code = header.status_code;
            world
                .insert(
                    packet_id,
                    (
                        sequence,
                        header,
                        Sent {
                            time: self.timer.elapsed(),
                        },
                    ),
                )
                .unwrap();
            // TODO(alex) 2021-05-02: Check if this packet has sequence > previous latest sent.
            world
                .insert(destination.host_id, (LatestSent { packet_id },))
                .unwrap();
            let event_id = world.spawn((SentPacketEvent {
                packet_id,
                status_code,
            },));
        }

        // TODO(alex) 2021-04-11: This will ignore some of the packets received, while the
        // socket isn't ready to send, other packets may arrive and take the
        // `LatestReceived` token, leaving some received packets in an ack limbo.
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
        // To achieve this, something must be done in the receiver side of things, more than
        // just strapping a `LatestReceived` marker. We need a list of packets to
        // ack.
        //
        // ADD(alex) 2021-04-20: The problem above is solved, but we have a new one now, we
        // could end up lagging behind in acks. If many packets are arriving, and we
        // can't reply acking each one as they come, the following logic will be
        // acking the packets in whatever ordering their events are stored in, if
        // this order is crescent, then `past_acks` would be irrelevant, and ack lag
        // occurs, if the order is random it might be even worse.
        //
        // ADD(alex) 2021-04-20: There must be a check to grab the `LatestReceived` and ack
        // based on it, and drop the lagged events. It's doing the `LatestReceived`
        // bit, but we're **not** dropping the lagged events.
        // let ack_diff = {
        //     let latest_ack = latest_sent_to_remote
        //         .as_ref()
        //         .map_or(0, |(_, header)| header.ack);
        //     let diff = remote_to_ack - latest_ack;
        //     diff
        // };

        // let past_acks = {
        //     let latest_past_acks = latest_sent_to_remote
        //         .as_ref()
        //         .map_or(0, |(_, header)| header.past_acks);

        //     (latest_past_acks << ack_diff) | 0b1
        // };

        // let connection_id = world
        //     .query_one::<&AckingConnection>(destination.host_id)
        //     .map_or(None, |mut query| query.get().map(|host| host.connection_id))
        //     .or_else(|| {
        //         world
        //             .query_one::<&Connected>(destination.host_id)
        //             .map_or(None, |mut query| query.get().map(|host| host.connection_id))
        //     });

        // TODO(alex) 2021-04-04: This is going to send only 1 packet, we're using `next` on the
        // iterator and not looping! This system is part of a `tick` that has to be constantly
        // called, the looping shouldn't be done here, I think.
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
}

fn get_sequence(world: &hecs::World, destination: &Destination) -> Sequence {
    let mut latest_query = world
        .query_one::<(&LatestSent,)>(destination.host_id)
        .unwrap();
    let sequence = match latest_query.get() {
        Some((latest_sent,)) => {
            let mut sequence_query = world
                .query_one::<(&Sequence,)>(latest_sent.packet_id)
                .unwrap();
            let (sequence,) = sequence_query.get().unwrap();
            sequence.clone()
        }
        // TODO(alex) 2021-05-08: Check if it's a valid status code for a new sequence value.
        None => unsafe { Sequence::new_unchecked(1) },
    };
    sequence
}

fn get_ack(world: &hecs::World, destination: &Destination) -> u32 {
    let ack = match world
        .query::<&AckRemotePacketEvent>()
        .iter()
        .find(|(ack_event_id, ack_event)| ack_event.destination_id == destination.host_id)
    {
        Some((ack_event_id, ack_event)) => world
            .query_one::<&Sequence>(ack_event.packet_id)
            .unwrap()
            .get()
            .unwrap()
            .get(),
        None => 0,
    };
    ack
}
