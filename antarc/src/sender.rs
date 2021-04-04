use std::{net::UdpSocket, time::Instant};

use hecs::{Entity, Ref, World};

use crate::{
    host::{
        AckingConnection, Address, AwaitingConnectionAck, Connected, Disconnected, HasConnectionId,
        Host, RequestingConnection,
    },
    packet::{
        header::Header, ConnectionAccepted, ConnectionDenied, ConnectionRequest, DataTransfer,
        Footer, Heartbeat, Packet, Payload, Received, ToSend, CONNECTION_REQUEST,
    },
};

#[derive(Debug)]
pub(crate) struct Destination {
    pub(crate) host_id: Entity,
}

pub(crate) fn system_sender(socket: &UdpSocket, timer: &Instant, world: &mut World) {
    if let Some(packet) = world
        .query::<(&Header, &Payload, &ToSend, &Destination, &Address)>()
        .iter()
        .next()
    {
        let (id, (header, payload, to_send, destination, address)) = packet;

        // TODO(alex) 2021-04-04: Is this the only way to chain map on error? Every new state that
        // has a `connection_id` would have to be inserted here.
        //
        // ADD(alex): 2021-04-04: Instead of chaining `match`es, just add a component indicating
        // that the `Host` has a connection id! Simple and efficient.
        let connection_id = world
            .get::<HasConnectionId>(destination.host_id)
            .map(|has_connection_id| has_connection_id.get())
            .ok();

        let (packet_bytes, footer) = Packet::encode(header, payload, connection_id).unwrap();
    }
}
