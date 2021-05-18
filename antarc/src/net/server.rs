use std::{
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
    time::{Duration, Instant},
};

use hecs::World;
use log::{debug, error};

use crate::{
    events::Event,
    host::{AwaitingConnectionAck, Connected, Disconnected, Host, RequestingConnection},
    net::{client::receiver, NetManager, NetworkResource},
    packet::{
        header::Header, ConnectionId, Encoded, Footer, Packet, PacketKind, Payload, Queued, Sent,
        Sequence, CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST, DATA_TRANSFER,
        HEARTBEAT,
    },
};

#[derive(Debug)]
pub struct Server {
    connection_id_tracker: ConnectionId,
    disconnected: Vec<Host<Disconnected>>,
    requesting_connection: Vec<Host<RequestingConnection>>,
    awaiting_connection_ack: Vec<Host<AwaitingConnectionAck>>,
    connected: Vec<Host<Connected>>,
}

impl NetManager<Server> {
    pub fn new_server(address: &SocketAddr) -> Self {
        let server = Server {
            connection_id_tracker: unsafe { ConnectionId::new_unchecked(1) },
            disconnected: Vec::with_capacity(8),
            requesting_connection: Vec::with_capacity(8),
            awaiting_connection_ack: Vec::with_capacity(8),
            connected: Vec::with_capacity(8),
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
        let mut new_events = Vec::with_capacity(8);

        self.network
            .poll
            .poll(&mut self.network.events, Some(Duration::from_millis(150)))
            .unwrap();

        for event in self.network.events.iter() {
            if event.is_readable() {
                self.events.push(Event::ReadyToReceive);
            }

            if event.is_writable() {
                self.events.push(Event::ReadyToSend);
            }
        }

        for event in self.events.drain(..) {
            match event {
                Event::ReadyToReceive => {
                    debug!("Handling ReadyToReceive");

                    // receiver(&mut self.buffer, &self.network.udp_socket, &self.timer);
                }
                Event::ReadyToSend => {
                    debug!("Handling ReadyToSend");
                    // TODO(alex) 2021-05-18: What to do here ends up being a big problem, and I
                    // can't think of a good solution.
                    //
                    // The packets have an address, so if we were to just send it here, everything
                    // would be fine, but how do we get the sequence? We need to find a matching
                    // address in one of the host vectors, which depend on the host state, and get
                    // the sequence from there. This reqires checking both the `Sent` and `Acked`
                    // lists of every kind of host.
                    //
                    // A similar thing would have to be done on the `Received` and `Retrieved` to
                    // get the correct ack value, not even talking about `past_acks`.
                    //
                    // ADD(alex) 2021-05-18: The server won't send the same packet to every client,
                    // for example, when client B requests a connection, then the server will send
                    // a connection accepted to B, but will send a data transfer to client A, which
                    // is already connected.
                    loop {
                        let encoded = match self.queued.pop() {
                            Some(queued) => {
                                debug!("There is a packet to queued {:#?}", queued);

                                match queued.kind {
                                    PacketKind::ConnectionRequest => {
                                        error!("Server cannot send connection request!");
                                        unreachable!();
                                    }
                                    PacketKind::ConnectionAccepted => {
                                        debug!("Server has a ConnectionAccepted queued.");

                                        match self
                                            .kind
                                            .requesting_connection
                                            .drain_filter(|client| client.address == queued.address)
                                            .next()
                                        {
                                            Some(client) => {
                                                debug!(
                                                    "Client requesting connection {:#?}.",
                                                    client
                                                );
                                            }
                                            None => {
                                                error!(
                                                    "No client with {:#?} requesting connection!",
                                                    queued.address
                                                );
                                                unreachable!();
                                            }
                                        }
                                    }
                                    PacketKind::ConnectionDenied => {
                                        debug!("Server has a ConnectionDenied queued.");
                                    }
                                    PacketKind::Ack(ack) => {
                                        debug!("Server has an Ack queued {:#?}.", ack);
                                    }
                                    PacketKind::DataTransfer => {
                                        debug!("Server has a DataTransfer queued.");
                                    }
                                    PacketKind::Heartbeat => {
                                        debug!("Server has a Heartbeat queued.");
                                    }
                                }
                                queued
                            }
                            None => {
                                debug!("No packet queued found");
                                todo!()
                            }
                        };
                    }
                }
                Event::SendConnectionRequest { address } => {
                    error!("Server cannot handle SendConnectionRequest!");
                    unreachable!();
                }
                Event::ReceivedConnectionRequest { address } => {
                    debug!("Handling ReceivedConnectionRequest from {:#?}", address);
                }
                Event::FailedEncodingPacket { queued } => {
                    error!("Handling event FailedEncodingPacket for {:#?}", queued);
                }
                Event::FailedSendingPacket { encoded } => {
                    error!("Handling event FailedSendingPacket for {:#?}", encoded);
                }
                Event::SentPacket { sent } => {
                    debug!("Handling SentPacket for {:#?}", sent);
                }
                Event::ReceivedPacket { received } => {
                    debug!("Handling ReceivedPacket for {:#?}", received);
                }
                Event::SendHeartbeat { address } => {
                    debug!("Handling SendHeartbeat to {:#?}", address);
                }
            }
        }

        self.events.append(&mut new_events);
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
