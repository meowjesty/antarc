use std::{
    io,
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
    time::{Duration, Instant},
};

use log::{debug, error, warn};

use super::SendTo;
use crate::{
    events::{AntarcError, FailureEvent, ReceiverEvent, SenderEvent},
    host::{AwaitingConnectionAck, Connected, Disconnected, Host, RequestingConnection},
    net::{NetManager, NetworkResource},
    packet::{
        header::{ConnectionRequest, DataTransfer, Header, Heartbeat},
        kind::PacketKind,
        payload::{self, Payload},
        queued::Queued,
        received::Received,
        sequence::Sequence,
        ConnectionId, Footer, Packet, Sent, CONNECTION_ACCEPTED, CONNECTION_DENIED,
        CONNECTION_REQUEST, DATA_TRANSFER, HEARTBEAT,
    },
};

pub type PacketId = u64;

const CHECK_ANTARC_QUEUE: Duration = Duration::from_millis(250);

// TODO(alex) [mid] 2021-06-08: Is this the right approach to reducing duplication?
#[macro_export]
macro_rules! send {
    ($self: expr, $host: expr, $bytes: expr, OnError -> $failed: expr ) => {{
        match $self.network.udp_socket.send_to(&$bytes, $host.address) {
            Ok(num_sent) => {
                debug_assert!(num_sent > 0);
                $host.after_send();
            }
            Err(fail) => {
                if fail.kind() == io::ErrorKind::WouldBlock {
                    warn!("Would block on send_to {:?}", fail);
                    $self.network.writable = false;
                }

                // NOTE(alex): Cannot use `bytes` here (or in any failure event), as
                // it could end up being a duplicated packet, sequence and ack are
                // only incremented when send is successful.
                $self.event_system.failures.push($failed);

                break;
            }
        }
    }};
}

#[derive(Debug)]
pub struct Server {
    last_antarc_queue_check: Duration,
    connection_id_tracker: ConnectionId,
}

impl NetManager<Server> {
    pub fn new_server(address: &SocketAddr) -> Self {
        let server = Server {
            last_antarc_queue_check: Duration::default(),
            connection_id_tracker: unsafe { ConnectionId::new_unchecked(1) },
        };
        let net_manager = NetManager::new(address, server);
        net_manager
    }

    // TODO(alex) 2021-05-23: Allow the user to specify the destination of these messages, we then
    // check if they're in the `connected` list, and queue the packets as normal.
    pub fn enqueue(&mut self, message: Vec<u8>) -> PacketId {
        let id = self.connection.packet_id_tracker;

        for host in self.connection.connected.iter() {
            let state = Queued {
                time: self.timer.elapsed(),
                destination: host.address,
            };
            let packet = Packet { id, state };

            self.event_system
                .sender
                .push(SenderEvent::QueuedDataTransfer {
                    packet,
                    payload: Payload(message.clone()),
                });
        }

        self.connection.packet_id_tracker += 1;

        id
    }

    // TODO(alex) 2021-05-17: It's probably a good idea to start working on this before going
    // further, to validate that the ideas I had so far are working. Make this the focus.
    pub fn tick(&mut self) -> Result<usize, String> {
        self.network
            .poll
            .poll(&mut self.network.events, Some(Duration::from_millis(150)))
            .unwrap();

        for event in self.network.events.iter() {
            if event.is_readable() {
                self.network.readable = true;
            }

            if event.is_writable() {
                self.network.writable = true;
            }
        }

        while self.network.readable {
            match self.network.udp_socket.recv_from(&mut self.buffer) {
                Ok((num_received, source)) => {
                    debug_assert!(num_received > 0);

                    let (packet, payload) = match Packet::decode(
                        self.connection.packet_id_tracker,
                        &self.buffer[..num_received],
                        source,
                        &self.timer,
                    ) {
                        Ok(decoded) => decoded,
                        Err(fail) => {
                            self.event_system
                                .failures
                                .push(FailureEvent::AntarcError(fail));
                            continue;
                        }
                    };

                    debug!("Received new packet {:#?} {:#?}.", packet, source);

                    self.event_system.receiver.push(ReceiverEvent::AckRemote {
                        sequence: packet.state.header.info.sequence,
                    });

                    match packet.state.header.info.status_code {
                        CONNECTION_REQUEST => {
                            self.event_system
                                .receiver
                                .push(ReceiverEvent::ConnectionRequest {
                                    packet: packet.into(),
                                });
                        }
                        DATA_TRANSFER => {
                            self.event_system
                                .receiver
                                .push(ReceiverEvent::DataTransfer {
                                    packet: packet.into(),
                                    payload,
                                });
                        }
                        HEARTBEAT => {
                            self.event_system.receiver.push(ReceiverEvent::Heartbeat {
                                packet: packet.into(),
                            });
                        }
                        invalid => {
                            error!("Server received invalid packet type {:#?}.", invalid);
                            todo!();
                        }
                    }

                    self.connection.packet_id_tracker += 1;
                }
                Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                    warn!("Would block on recv_from {:?}", fail);
                    self.network.readable = false;
                }
                Err(fail) => {
                    warn!("Failed recv_from with {:?}", fail);

                    self.event_system.failures.push(FailureEvent::IO(fail));

                    self.network.readable = false;
                }
            }
        }

        // TODO(alex) [mid] 2021-05-27: Send a heartbeat, look at the connected hosts only.
        for host in self.connection.connected.iter() {
            let threshold = host.state.latest_sent_time + Duration::from_millis(1500);
            let time_expired = threshold < self.timer.elapsed();
            if time_expired {
                debug!(
                    "Time expired, sending host a heartbeat {:#?}.",
                    host.address
                );
                self.event_system.sender.push(SenderEvent::QueuedHeartbeat {
                    address: host.address,
                });
            }
        }

        // TODO(alex) [high] 2021-06-06: Finally de-duplicate this code.
        while self.network.writable
            && self.event_system.sender.is_empty() == false
            && self.connection.is_empty() == false
        {
            debug_assert!(self.event_system.sender.len() > 0);

            for event in self.event_system.sender.drain(..1) {
                match event {
                    SenderEvent::QueuedDataTransfer { packet, payload } => {
                        debug!("Handling QueuedDataTransfer {:#?}.", packet);

                        let destination = packet.state.destination;
                        let connected = self
                            .connection
                            .connected
                            .iter_mut()
                            .find(|host| host.address == destination)
                            .unwrap();

                        let (header, bytes, footer) = connected.prepare_data_transfer(&payload);
                        send!(
                            self,
                            connected,
                            bytes,
                            OnError -> FailureEvent::SendDataTransfer {
                                packet,
                                payload
                            }
                        );

                        let sent =
                            packet.to_sent(header, footer, self.timer.elapsed(), destination);
                        connected.state.sent_data_transfers.push(sent);
                    }
                    SenderEvent::QueuedConnectionAccepted { packet } => {
                        debug!("Handling QueuedConnectionAccepted {:#?}.", packet);

                        // TODO(alex) [mid] 2021-06-08: A `HashMap<SocketAddr, Host<State>>` is
                        // probably more appropriate, as this find address is pertinent.
                        // NOTE(alex): If `DrainIter` is used here, then `send!` would require a way
                        // of putting the host back into the proper list.
                        let (index, requesting_connection) = self
                            .connection
                            .requesting_connection
                            .iter_mut()
                            .enumerate()
                            .find(|(_, host)| host.address == packet.state.destination)
                            .unwrap();

                        let connection_id = self.net_type.connection_id_tracker;
                        let (_, bytes, _) =
                            requesting_connection.prepare_connection_accepted(connection_id);

                        send!(self, requesting_connection, bytes,
                            OnError -> FailureEvent::SendConnectionAccepted { packet });

                        let requesting_connection =
                            self.connection.requesting_connection.remove(index);
                        let host = requesting_connection.await_connection();
                        self.connection.awaiting_connection_ack.push(host);

                        self.net_type.connection_id_tracker =
                            NonZeroU16::new(self.net_type.connection_id_tracker.get() + 1).unwrap();
                    }
                    SenderEvent::QueuedConnectionRequest { .. } => {
                        error!("Server cannot handle SendConnectionRequest!");
                        unreachable!();
                    }
                    SenderEvent::QueuedHeartbeat { address } => {
                        debug!("Handling QueuedHeartbeat to {:#?}.", address);

                        let connected = self
                            .connection
                            .connected
                            .iter_mut()
                            .find(|host| host.address == address)
                            .unwrap();

                        let address = connected.address;
                        let (header, bytes, footer) = connected.prepare_heartbeat();

                        send!(self, connected, bytes,
                            OnError -> FailureEvent::SendHeartbeat { address });

                        // TODO(alex) [vhigh] 2021-06-13: Don't think I need to keep a list of every
                        // type of packet, to ack packets that are not data transfers I could just
                        // keep a `not_acked: Vec<HeaderInfo>` just to get access to sequence and
                        // timer values.
                        //
                        // Hmm, as it stands, it would be something pretty similar to what I'm doing
                        // already, so maybe I'm trying to solve a non-problem.
                        let sent = Packet::sent_heartbeat(
                            self.connection.packet_id_tracker,
                            header,
                            footer,
                            self.timer.elapsed(),
                            address,
                        );
                        connected.state.sent_heartbeats.push(sent);
                    }
                }
            }
        }

        // TODO(alex) [low] 2021-05-26: Check `fn _received_connection_request` notes.
        for event in self.event_system.receiver.drain(..) {
            match event {
                ReceiverEvent::ConnectionRequest { packet } => {
                    debug!("Handling received connection request for {:#?}", packet);

                    let source = packet.state.source;

                    // TODO(alex) [low] 2021-05-25: Is there a way to better handle this case?
                    if self.connection.is_known(&source) {
                        error!("Host is already in another state to receive this packet!");
                        continue;
                    }

                    debug!("Unknown address, creating new host.");

                    // TODO(alex) [low] 2021-05-26: Always creating a new host here, as I've dropped
                    // the concept of a `Disconnected` host, this could be revisited later when the
                    // design is clearer.
                    let host = Host::received_connection_request(source);

                    let packet_id = self.connection.packet_id_tracker;
                    let packet = Packet::new(packet_id, self.timer.elapsed(), host.address);
                    let connection_accepted = SenderEvent::QueuedConnectionAccepted { packet };
                    self.event_system.sender.push(connection_accepted);

                    self.connection.requesting_connection.push(host);
                    self.connection.packet_id_tracker += 1;
                }
                ReceiverEvent::AckRemote { .. } => {}
                ReceiverEvent::DataTransfer { packet, payload } => {
                    debug!("Handling received data transfer for {:#?}", packet);

                    let source = packet.state.source;
                    debug_assert_eq!(self.connection.is_requesting_connection(&source), false);

                    if let Some(host) = self
                        .connection
                        .awaiting_connection_ack
                        .drain_filter(|h| {
                            // TODO(alex) [low] 2021-05-27: Change this hardcoded check for the
                            // connection accepted sent sequence (1).
                            h.address == source && packet.state.header.info.ack == 1
                        })
                        .next()
                    {
                        debug!("Host was awaiting connection ack.");
                        let connection_id = packet.state.footer.connection_id.unwrap();
                        let host = host.connection_accepted(connection_id);

                        self.connection.connected.push(host);
                    }

                    if let Some(host) = self
                        .connection
                        .connected
                        .iter_mut()
                        .find(|h| h.address == source)
                    {
                        debug!("Updating connected host with data transfer.");

                        // TODO(alex) [low] 2021-05-27: Need a mechanism to ack out of order
                        // packets, right now we just don't update the ack trackers if they come
                        // from a lower value than what the host has.
                        host.local_ack_tracker = (packet.state.header.info.ack
                            > host.local_ack_tracker)
                            .then(|| packet.state.header.info.ack)
                            .unwrap_or(host.local_ack_tracker);

                        let remote_sequence = packet.state.header.info.sequence.get();
                        host.remote_ack_tracker = (remote_sequence > host.remote_ack_tracker)
                            .then(|| packet.state.header.info.sequence.get())
                            .unwrap_or(host.remote_ack_tracker);

                        host.state.recv_data_transfers.push(packet);

                        self.retrievable_count += 1;
                    }
                }
                ReceiverEvent::Heartbeat { packet } => {
                    debug!("Handling received heartbeat for {:#?}", packet);

                    let source = packet.state.source;
                    debug_assert_eq!(self.connection.is_requesting_connection(&source), false);

                    if let Some(host) = self
                        .connection
                        .awaiting_connection_ack
                        .drain_filter(|h| {
                            // TODO(alex) [low] 2021-05-27: Change this hardcoded check for the
                            // connection accepted sent sequence (1).
                            h.address == source && packet.state.header.info.ack == 1
                        })
                        .next()
                    {
                        debug!("Host was awaiting connection ack.");
                        let connection_id = packet.state.footer.connection_id.unwrap();
                        let host = host.connection_accepted(connection_id);

                        self.connection.connected.push(host);
                    }

                    if let Some(host) = self
                        .connection
                        .connected
                        .iter_mut()
                        .find(|h| h.address == source)
                    {
                        debug!("Updating connected host with heartbeat.");

                        // TODO(alex) [low] 2021-05-27: Need a mechanism to ack out of order
                        // packets, right now we just don't update the ack trackers if they come
                        // from a lower value than what the host has.
                        host.local_ack_tracker = (packet.state.header.info.ack
                            > host.local_ack_tracker)
                            .then(|| packet.state.header.info.ack)
                            .unwrap_or(host.local_ack_tracker);

                        let remote_sequence = packet.state.header.info.sequence.get();
                        host.remote_ack_tracker = (remote_sequence > host.remote_ack_tracker)
                            .then(|| packet.state.header.info.sequence.get())
                            .unwrap_or(host.remote_ack_tracker);

                        host.state.recv_heartbeats.push(packet);
                    }
                }
            }
        }

        for error in self.event_system.failures.drain(..) {
            match error {
                FailureEvent::AntarcError(fail) => {
                    error!("Failed internally with {:#?}.", fail);
                }
                FailureEvent::IO(fail) => {
                    error!("Failed with IO error {:#?}.", fail);
                }
                FailureEvent::SendDataTransfer { .. } => todo!(),
                FailureEvent::SendConnectionAccepted { .. } => todo!(),
                FailureEvent::SendConnectionRequest => unreachable!(),
                FailureEvent::SendHeartbeat { .. } => todo!(),
            }
        }

        Ok(self.retrievable_count)
    }

    // TODO(alex) [vlow] 2021-06-13: It might be a good idea to return a bit more information than
    // just `ConnectionId`, such as a sequence, or maybe time received, so that the user may know
    // which payload is the "freshest".
    pub fn retrieve(&mut self) -> Vec<(ConnectionId, Vec<u8>)> {
        debug!("Retrieve for server.");
        self.retrievable_count = 0;
        let retrievable = self.retrievable.drain(..).collect::<Vec<_>>();
        retrievable
    }
}
