use std::{net::UdpSocket, time::Instant};

use hecs::{With, Without, World};

use super::RequestingConnection;
use crate::{
    host::{Address, Host},
    packet::{
        header::Header, ConnectionRequest, DataTransfer, Internal, Packet, Received, Sequence,
    },
};

pub(crate) fn system_new_host_handler(world: &mut World) {
    let mut new_hosts = Vec::with_capacity(4);

    for (packet, (address, header, _)) in world
        // NOTE(alex) 2021-03-25: Only the freshly received packets, we don't even know if they have
        // a known host.
        .query::<Without<Host, (&Address, &Header, &Received)>>()
        .iter()
    {
        assert_eq!(header.sequence.get(), 1);
        // TODO(alex) 2021-03-25: Validate the header values, if this is a new host it should
        // respect `Header` connection request initialization.
        let host = Host {
            ack_tracker: header.sequence.get(),
            ..Default::default()
        };
        new_hosts.push((packet, host));
    }

    while let Some((packet, host)) = new_hosts.pop() {
        let host_id = world.spawn((host,));
        // TODO(alex) 2021-03-25: Associate packet <-> host.
        // hecs appears to not have the same concept of `pushing` components into an entity, so this
        // must be done "manually"? Do I need to add `Vec<Packet>` to the `Host` in order to achieve
        // this?
    }
}
