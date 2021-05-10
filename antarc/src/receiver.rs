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
use hecs::Entity;
use log::debug;

use crate::{
    events::{AckLocalPacketEvent, ReceivedNewPacketEvent},
    host::Address,
    net::{NetManager, NetworkResource},
    packet::{header::Header, Acked, Footer, Packet, Payload, Received, Sent, Sequence},
    readiness::Readable,
    sender::Destination,
};

#[derive(Debug, Clone, Copy, Hash, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Source {
    pub(crate) host_id: Entity,
}

impl From<Destination> for Source {
    fn from(destination: Destination) -> Self {
        Source {
            host_id: destination.host_id,
        }
    }
}

impl PartialEq<Destination> for Source {
    fn eq(&self, other: &Destination) -> bool {
        other.host_id == self.host_id
    }
}

impl<T> NetManager<T> {
    ///
    /// - Raises the `ReceivedNewPacket` event.
    pub(crate) fn receiver(&mut self) {
        let (buffer, world, timer) = (&mut self.buffer, &mut self.world, &self.timer);

        let mut handled_events = Vec::with_capacity(8);
        let mut received_new_packet = None;

        if let Some((event_id, readable)) = world.query::<&Readable>().iter().next() {
            debug!("receiver -> handle Readable {:#?}", event_id);

            if let Some((resource_id, resource)) =
                world.query::<&mut NetworkResource>().iter().next()
            {
                debug!("receiver -> get NetworkResource {:#?}", resource_id);

                let socket = &mut resource.socket;

                match socket.recv_from(buffer) {
                    Ok((num_recv, from_addr)) => {
                        debug!(
                            "receiver -> received a packet {:?} with {:#?} bytes",
                            &buffer[0..18],
                            num_recv
                        );

                        if num_recv > 0 {
                            let remote_address = Address(from_addr);

                            let (sequence, header, payload, footer) =
                                Packet::decode(&buffer[0..num_recv]).unwrap();

                            let received = Received {
                                time: timer.elapsed(),
                            };
                            let packet =
                                (sequence, header, payload, footer, remote_address, received);
                            received_new_packet = Some(packet);
                        } else {
                            eprintln!("Received 0 bytes from {:#?}.", from_addr);
                            unreachable!();
                        }
                    }
                    Err(fail) => {
                        eprintln!("Failed to receive on socket with {:#?}.", fail);
                    }
                }
                handled_events.push(event_id);
            }
        }

        if let Some((sequence, header, payload, footer, address, received)) = received_new_packet {
            type ReceivedPacket = (Sequence, Header, Payload, Footer, Address, Received);
            let received_packet = (sequence, header, payload, footer, address, received);
            let packet_id = world.spawn(received_packet);
            debug!("receiver -> spawning packet received {:#?}", packet_id);

            let event_id = world.spawn((ReceivedNewPacketEvent { packet_id },));
            debug!(
                "receiver -> spawning ReceivedNewPacketEvent {:#?}",
                event_id
            );
        }

        while let Some(event_id) = handled_events.pop() {
            let _ = world.despawn(event_id).unwrap();
            debug!("receiver -> despawning Readable {:#?}", event_id);
        }
    }

    // TODO(alex) 2021-04-08: `system_received_heartbeat`.

    /// NOTE(alex): System that acks packets sent by the local host to a remote host (acks `Sent`).
    // TODO(alex): 2021-04-21: Handle this event properly, with the updated way.
    pub(crate) fn system_on_received_ack_sent_packet(&mut self) {
        let (buffer, world, timer) = (&mut self.buffer, &mut self.world, &self.timer);

        let mut handled_events = Vec::with_capacity(8);
        let mut sent_to_ack_list = Vec::with_capacity(8);

        for (event_id, (event,)) in world.query::<(&AckLocalPacketEvent,)>().iter() {
            let AckLocalPacketEvent {
                packet_id,
                source_id: source,
            } = event;

            let mut acker_query = world.query_one::<(&Header,)>(*packet_id).unwrap();
            let (received,) = acker_query.get().unwrap();

            // TODO(alex) 2021-04-14: Handle `past_acks`. Should this be done here or raise another
            // event?
            //
            // NOTE(alex): Search for packets that have been `Sent`, but were not `Acked` yet, that
            // belong to this specific `Host` (via `Destination`) and that have a matching
            // `sequence` (sent) to this packet's `ack` (received).
            if let Some(sent) = world
                .query::<(&Sequence, &Header, &Sent, &Destination)>()
                .without::<Acked>()
                .iter()
                .find_map(|(sent_id, (sequence, header, sent, destination))| {
                    (destination.host_id == *source && sequence.get() == received.ack)
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
