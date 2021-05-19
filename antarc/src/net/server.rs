use std::{
    io,
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
    time::{Duration, Instant},
};

use log::{debug, error, warn};

use super::SendTo;
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

type PacketId = u64;

#[derive(Debug)]
pub struct Server {
    id_tracker: PacketId,
    received_tracker: usize,
    connection_id_tracker: ConnectionId,
    disconnected: Vec<Host<Disconnected>>,
    requesting_connection: Vec<Host<RequestingConnection>>,
    awaiting_connection_ack: Vec<Host<AwaitingConnectionAck>>,
    connected: Vec<Host<Connected>>,
}

impl NetManager<Server> {
    pub fn new_server(address: &SocketAddr) -> Self {
        let server = Server {
            id_tracker: 0,
            connection_id_tracker: unsafe { ConnectionId::new_unchecked(1) },
            disconnected: Vec::with_capacity(8),
            requesting_connection: Vec::with_capacity(8),
            awaiting_connection_ack: Vec::with_capacity(8),
            connected: Vec::with_capacity(8),
            received_tracker: 0,
        };
        let net_manager = NetManager::new(address, server);
        net_manager
    }

    pub fn enqueue(&mut self, message: Vec<u8>) -> PacketId {
        let id = self.kind.id_tracker;

        let state = Queued {
            time: self.timer.elapsed(),
        };
        let packet = Packet {
            id,
            payload: Payload(message),
            state,
            kind: PacketKind::DataTransfer,
        };

        self.queued.push((SendTo::All, packet));
        self.kind.id_tracker += 1;

        id
    }

    pub fn cancel_packet(&mut self, packet_id: PacketId) -> bool {
        self.queued
            .drain_filter(|(_, queued)| queued.id == packet_id)
            .next()
            .is_some()
    }

    // TODO(alex) 2021-05-17: It's probably a good idea to start working on this before going
    // further, to validate that the ideas I had so far are working. Make this the focus.
    pub fn tick(&mut self) -> Result<usize, String> {
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

                    loop {
                        match receiver(
                            self.kind.id_tracker,
                            &mut self.buffer,
                            &self.network.udp_socket,
                            &self.timer,
                        ) {
                            Ok(received) => {
                                debug!("Received new packet {:#?}.", received);
                                new_events.push(Event::ReceivedPacket { received });
                                self.kind.id_tracker += 1;
                                self.kind.received_tracker += 1;
                            }
                            Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                                warn!("Would block on recv_from {:?}", fail);
                                break;
                            }
                            Err(fail) => {
                                warn!("Failed recv_from with {:?}", fail);
                                break;
                            }
                        }
                    }
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
                        if let Some((send_to, queued)) = self.queued.drain(..1).next() {
                            match send_to {
                                SendTo::All => {
                                    debug!("Sending packet to all.");

                                    // TODO(alex) 2021-05-18: I need a way to mark that this packet
                                    // was sent to this host, to avoid sending duplicates.
                                    //
                                    // ADD(alex) 2021-05-18: This could possibly be done by looking
                                    // up at the packet's sequence and the host's
                                    // `sequence_tracker`.
                                    let payload_length = queued.payload.len();
                                    for connected in self.kind.connected.iter_mut() {
                                        let sequence = connected.sequence_tracker;
                                        let ack = connected.ack_tracker;
                                        let connection_id = connected.state.connection_id;
                                    }
                                }
                                SendTo::Single(address) => {
                                    debug!("Sending packet to single address {:#?}.", address);
                                }
                            }
                        } else {
                            // TODO(alex) 2021-05-18: No packets queued, send heartbeat!
                            //
                            // TODO(alex) 2021-05-18: Only send this heartbeat if we received some
                            // packet from the server, and we have not sent an ack for it yet.
                            // new_events.push(Event::SendHeartbeat { address });
                        }
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
        Ok(self.kind.received_tracker)
    }

    pub fn retrieve(&mut self) -> Vec<(ConnectionId, Vec<u8>)> {
        self.kind.received_tracker = 0;
        todo!()
    }
}
