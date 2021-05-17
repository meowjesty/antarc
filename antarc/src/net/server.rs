use std::{
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
    time::{Duration, Instant},
};

use hecs::World;
use log::{debug, error};

use crate::{
    net::{NetManager, NetworkResource},
    packet::{
        header::Header, ConnectionId, Footer, Payload, Queued, Sent, Sequence,
        CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST, DATA_TRANSFER, HEARTBEAT,
    },
};

#[derive(Debug)]
pub struct Server {
    connection_id_tracker: ConnectionId,
}

impl NetManager<Server> {
    pub fn new_server(address: &SocketAddr) -> Self {
        let server = Server {
            connection_id_tracker: unsafe { ConnectionId::new_unchecked(1) },
        };
        let net_manager = NetManager::new(address, server);
        net_manager
    }

    /// TODO(alex) 2021-03-08: We need an API like `get('/{:id}')` route, but for `Host`s.
    pub fn listen(&mut self) -> () {
        todo!()
    }

    // TODO(alex) 2021-05-17: It's probably a good idea to start working on this before going
    // further, to validate that the ideas I had so far are working. Make this the focus.
    pub fn tick(&mut self) -> () {
        todo!()
    }

    /// System responsible for attributing a `Source` host to a packet entity, if the host matches
    /// the packet's `Address`, otherwise it raises the `OnReceivedConnectionRequest` event (if
    /// the `Header` represents a `ConnectionRequest`).
    ///
    /// Also handles the changes to a host's `LatestReceived` packet.
    ///
    /// - Raises the `OnReceivedConnectionRequest` event.
    pub(crate) fn on_received_new_packet(&mut self) {
        todo!()
    }

    // NOTE(alex): This handler is specific for the server, as the client doesn't receive connection
    // requests, at least not yet.
    pub(crate) fn on_received_connection_request(&mut self) {
        todo!()
    }

    pub(crate) fn on_sent_packet(&mut self) {
        todo!()
    }

    // TODO(alex) 2021-04-27: Handle the `RequestingConnection` host state, we get the connection
    // request, create a host (if one does not exist with the same address already), but nothing is
    // being done to actually send back a connection accepted (or denied). I should start handling
    // only the accepted case, leave the option to deny a connection to later. I'm thinking about
    // this connection handler mechanism as being a simple syn/ack, and leaving proper connection
    // management to the user, so a connection denied would only be sent if a host belongs to a ban
    // list that the user has created. This means that the first time the connection will always be
    // replied with accepted.

    pub(crate) fn on_sent_connection_accepted(&mut self) {
        todo!()
    }

    pub fn retrieve(&self) -> Vec<(u32, Vec<u8>)> {
        todo!();
    }

    // pub fn enqueue(&self, data: Vec<u8>) {
    //     todo!()
    // }

    // pub fn ban_host(&self, host_id: u32) {
    //     todo!();
    // }
}
