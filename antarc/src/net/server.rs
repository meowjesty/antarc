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
    host::{AwaitingConnectionAck, Connected, Disconnected, Host, HostState, RequestingConnection},
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

#[derive(Debug)]
pub struct Server {
    id_tracker: PacketId,
    last_antarc_queue_check: Duration,
    connection_id_tracker: ConnectionId,
    // TODO(alex) [low] 2021-05-25: Why would I need this kind of host?
    // disconnected: Vec<Host<Disconnected>>,
    requesting_connection: Vec<Host<RequestingConnection>>,
    awaiting_connection_ack: Vec<Host<AwaitingConnectionAck>>,
    connected: Vec<Host<Connected>>,
}

impl NetManager<Server> {
    pub fn new_server(address: &SocketAddr) -> Self {
        let server = Server {
            id_tracker: 0,
            last_antarc_queue_check: Duration::default(),
            connection_id_tracker: unsafe { ConnectionId::new_unchecked(1) },
            requesting_connection: Vec::with_capacity(8),
            awaiting_connection_ack: Vec::with_capacity(8),
            connected: Vec::with_capacity(8),
        };
        let net_manager = NetManager::new(address, server);
        net_manager
    }

    // TODO(alex) 2021-05-23: Allow the user to specify the destination of these messages, we then
    // check if they're in the `connected` list, and queue the packets as normal.
    pub fn enqueue(&mut self, message: Vec<u8>) -> PacketId {
        let id = self.kind.id_tracker;

        for host in self.kind.connected.iter() {
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

        self.kind.id_tracker += 1;

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
                        self.kind.id_tracker,
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
                            let header = Header {
                                info: packet.state.header.info,
                                kind: ConnectionRequest,
                            };
                            let state = Received {
                                header,
                                footer: packet.state.footer,
                                time: packet.state.time,
                                source: packet.state.source,
                            };
                            let packet = Packet {
                                id: packet.id,
                                state,
                            };

                            self.event_system
                                .receiver
                                .push(ReceiverEvent::ConnectionRequest { packet });
                        }
                        DATA_TRANSFER => {
                            let header = Header {
                                info: packet.state.header.info,
                                kind: DataTransfer { payload },
                            };
                            let state = Received {
                                header,
                                footer: packet.state.footer,
                                time: packet.state.time,
                                source: packet.state.source,
                            };
                            let packet = Packet {
                                id: packet.id,
                                state,
                            };

                            self.event_system
                                .receiver
                                .push(ReceiverEvent::DataTransfer { packet });
                        }
                        HEARTBEAT => {
                            let header = Header {
                                info: packet.state.header.info,
                                kind: Heartbeat,
                            };
                            let state = Received {
                                header,
                                footer: packet.state.footer,
                                time: packet.state.time,
                                source: packet.state.source,
                            };
                            let packet = Packet {
                                id: packet.id,
                                state,
                            };

                            self.event_system
                                .receiver
                                .push(ReceiverEvent::Heartbeat { packet });
                        }
                        invalid => {
                            error!("Server received invalid packet type {:#?}.", invalid);
                            todo!();
                        }
                    }

                    self.kind.id_tracker += 1;
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
        for host in self.kind.connected.iter() {
            let threshold = host.state.time_last_sent + Duration::from_millis(1500);
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

        let has_host = self.kind.requesting_connection.len() > 0
            || self.kind.awaiting_connection_ack.len() > 0
            || self.kind.connected.len() > 0;

        // TODO(alex) [high] 2021-06-06: Finally de-duplicate this code.
        //
        // ADD(alex) [high] 2021-06-07: Refactored part of the header and encoding into `Host`
        // functions, but having a hard time figuring out how to refactor the more type dependent
        // things. Like, how do I refactor the send match, if each send is different?
        while self.network.writable && has_host && self.event_system.sender.len() > 0 {
            debug_assert!(self.event_system.sender.len() > 0);

            for event in self.event_system.sender.drain(..1) {
                match event {
                    SenderEvent::QueuedDataTransfer { packet, payload } => {
                        debug!("Handling QueuedDataTransfer {:#?}.", packet);

                        let connected = self
                            .kind
                            .connected
                            .iter_mut()
                            .find(|host| host.address == packet.state.destination)
                            .unwrap();

                        let (header, bytes, footer) = connected.prepare_data_transfer(payload);
                        match self.network.udp_socket.send_to(&bytes, connected.address) {
                            Ok(num_sent) => {
                                debug_assert!(num_sent > 0);
                                connected.sequence_tracker =
                                    Sequence::new(connected.sequence_tracker.get() + 1).unwrap();
                            }
                            Err(fail) => {
                                if fail.kind() == io::ErrorKind::WouldBlock {
                                    warn!("Would block on send_to {:?}", fail);
                                    self.network.writable = false;
                                }

                                // NOTE(alex): Cannot use `bytes` here (or in any failure event), as
                                // it could end up being a duplicated packet, sequence and ack are
                                // only incremented when send is successful.
                                let failed = FailureEvent::SendDataTransfer {
                                    packet,
                                    payload: header.kind.payload,
                                };
                                self.event_system.failures.push(failed);

                                break;
                            }
                        }
                    }
                    SenderEvent::QueuedConnectionAccepted { packet } => {
                        debug!("Handling QueuedConnectionAccepted {:#?}.", packet);

                        let requesting_connection = self
                            .kind
                            .requesting_connection
                            .drain_filter(|host| host.address == packet.state.destination)
                            .next()
                            .unwrap();

                        let connection_id = self.kind.connection_id_tracker;
                        let destination = requesting_connection.address;
                        let (_, bytes, _) =
                            requesting_connection.prepare_connection_accepted(connection_id);

                        match self.network.udp_socket.send_to(&bytes, destination) {
                            Ok(num_sent) => {
                                debug_assert!(num_sent > 0);

                                let state = AwaitingConnectionAck {
                                    attempts: 0,
                                    connection_id: self.kind.connection_id_tracker,
                                    last_sent: 1,
                                };
                                let host = Host {
                                    sequence_tracker: unsafe {
                                        Sequence::new_unchecked(
                                            requesting_connection.sequence_tracker.get() + 1,
                                        )
                                    },
                                    remote_ack_tracker: 1,
                                    local_ack_tracker: requesting_connection.local_ack_tracker,
                                    address: requesting_connection.address,
                                    received: requesting_connection.received,
                                    state,
                                };
                                self.kind.awaiting_connection_ack.push(host);

                                self.kind.connection_id_tracker =
                                    NonZeroU16::new(self.kind.connection_id_tracker.get() + 1)
                                        .unwrap();
                            }
                            Err(fail) => {
                                error!(
                                    "Server failed sending packet {:#?} to {:#?}.",
                                    fail, destination
                                );
                                if fail.kind() == io::ErrorKind::WouldBlock {
                                    warn!("Would block on send_to {:?}", fail);
                                    self.network.writable = false;
                                }
                                self.kind.requesting_connection.push(requesting_connection);

                                let failed = FailureEvent::SendConnectionAccepted { packet };
                                self.event_system.failures.push(failed);

                                break;
                            }
                        }
                    }
                    SenderEvent::QueuedConnectionRequest { .. } => {
                        error!("Server cannot handle SendConnectionRequest!");
                        unreachable!();
                    }
                    SenderEvent::QueuedHeartbeat { address } => {
                        debug!("Handling QueuedHeartbeat to {:#?}.", address);

                        let connected = self
                            .kind
                            .connected
                            .iter_mut()
                            .find(|host| host.address == address)
                            .unwrap();

                        let destination = connected.address;
                        let (_, bytes, _) = connected.prepare_heartbeat();

                        match self.network.udp_socket.send_to(&bytes, destination) {
                            Ok(num_sent) => {
                                debug_assert!(num_sent > 0);

                                connected.sequence_tracker =
                                    Sequence::new(connected.sequence_tracker.get() + 1).unwrap();
                                self.kind.id_tracker += 1;
                            }
                            Err(fail) => {
                                error!(
                                    "Server failed sending packet {:#?} to {:#?}.",
                                    fail, destination
                                );

                                if fail.kind() == io::ErrorKind::WouldBlock {
                                    warn!("Would block on send_to {:?}", fail);
                                    self.network.writable = false;
                                }

                                let failed = FailureEvent::SendHeartbeat { address };
                                self.event_system.failures.push(failed);

                                break;
                            }
                        }
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
                    if self
                        .kind
                        .requesting_connection
                        .iter()
                        .any(|h| h.address == source)
                        || self.kind.connected.iter().any(|h| h.address == source)
                    {
                        error!("Host is already in another state to receive this packet!");
                        continue;
                    }

                    debug!("Unknown address, creating new host.");

                    // TODO(alex) [low] 2021-05-26: Always creating a new host here, as I've dropped
                    // the concept of a `Disconnected` host, this could be revisited later when the
                    // design is clearer.
                    let ack_tracker = packet.state.header.info.sequence.get();
                    let state = RequestingConnection { attempts: 0 };
                    let host = Host {
                        sequence_tracker: Sequence::default(),
                        remote_ack_tracker: ack_tracker,
                        local_ack_tracker: 0,
                        address: source,
                        received: Vec::with_capacity(128),
                        state,
                    };

                    let id = self.kind.id_tracker;
                    let state = Queued {
                        time: self.timer.elapsed(),
                        destination: host.address,
                    };
                    let packet = Packet { id, state };
                    let connection_accepted = SenderEvent::QueuedConnectionAccepted { packet };

                    self.kind.requesting_connection.push(host);
                    self.event_system.sender.push(connection_accepted);
                    self.kind.id_tracker += 1;
                }
                ReceiverEvent::AckRemote { .. } => {}
                ReceiverEvent::DataTransfer { packet } => {
                    debug!("Handling received data transfer for {:#?}", packet);
                    if self
                        .kind
                        .requesting_connection
                        .iter()
                        .any(|h| h.address == packet.state.source)
                    {
                        error!(
                            "Host is requesting connection , invalid for data transfer {:#?}!",
                            packet
                        );
                        unreachable!();
                    }

                    if let Some(host) = self
                        .kind
                        .awaiting_connection_ack
                        .drain_filter(|h| {
                            // TODO(alex) [low] 2021-05-27: Change this hardcoded check for the
                            // connection accepted sent sequence (1).
                            h.address == packet.state.source && packet.state.header.info.ack == 1
                        })
                        .next()
                    {
                        debug!("Host was awaiting connection ack.");
                        let connection_id = host.state.connection_id;
                        let last_sent = host.state.last_sent;
                        let state = Connected {
                            connection_id,
                            rtt: Duration::default(),
                            last_sent,
                            time_last_sent: Duration::default(),
                        };
                        let sequence_tracker = host.sequence_tracker;
                        let ack_tracker = host.remote_ack_tracker;
                        let last_acked = host.local_ack_tracker;
                        let address = host.address;
                        let received = host.received;
                        let host = Host {
                            sequence_tracker,
                            remote_ack_tracker: ack_tracker,
                            local_ack_tracker: last_acked,
                            address,
                            received,
                            state,
                        };

                        self.kind.connected.push(host);
                    }

                    if let Some(host) = self
                        .kind
                        .connected
                        .iter_mut()
                        .find(|h| h.address == packet.state.source)
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

                        host.received.push(packet);

                        self.retrievable_count += 1;
                    }
                }
                ReceiverEvent::Heartbeat { packet } => {
                    debug!("Handling received heartbeat for {:#?}", packet);
                    if self
                        .kind
                        .requesting_connection
                        .iter()
                        .any(|h| h.address == packet.state.source)
                    {
                        error!(
                            "Host is requesting connection, invalid for heartbeat {:#?}!",
                            packet
                        );
                        unreachable!();
                    }

                    if let Some(host) = self
                        .kind
                        .awaiting_connection_ack
                        .drain_filter(|h| {
                            // TODO(alex) [low] 2021-05-27: Change this hardcoded check for the
                            // connection accepted sent sequence (1).
                            h.address == packet.state.source && packet.state.header.info.ack == 1
                        })
                        .next()
                    {
                        debug!("Host was awaiting connection ack.");
                        let connection_id = host.state.connection_id;
                        let last_sent = host.state.last_sent;
                        let state = Connected {
                            connection_id,
                            rtt: Duration::default(),
                            last_sent,
                            time_last_sent: Duration::default(),
                        };
                        let sequence_tracker = host.sequence_tracker;
                        let ack_tracker = host.remote_ack_tracker;
                        let last_acked = host.local_ack_tracker;
                        let address = host.address;
                        let received = host.received;
                        let host = Host {
                            sequence_tracker,
                            remote_ack_tracker: ack_tracker,
                            local_ack_tracker: last_acked,
                            address,
                            received,
                            state,
                        };

                        self.kind.connected.push(host);
                    }

                    if let Some(host) = self
                        .kind
                        .connected
                        .iter_mut()
                        .find(|h| h.address == packet.state.source)
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

                        // host.received.push(packet);
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

    pub fn retrieve(&mut self) -> Vec<(ConnectionId, Vec<Vec<u8>>)> {
        debug!("Retrieve for server.");
        self.retrievable_count = 0;
        let retrievable = self
            .retrievable
            .drain()
            .map(|(k, v)| (k, v))
            .collect::<Vec<_>>();
        retrievable
    }
}

fn prepare_connection_accepted(connection_id: ConnectionId, destination: SocketAddr) {
    let ack = 1;
    let header = Header::connection_accepted(ack);
    let payload = Payload::default();
    let (bytes, footer) = Packet::encode(&payload, &header.info, Some(connection_id));
}

fn prepare_data_transfer1(
    sequence: Sequence,
    ack: u32,
    connection_id: Option<ConnectionId>,
    payload: Payload,
) -> (Header<DataTransfer>, Vec<u8>, Footer) {
    let header = Header::data_transfer(sequence, ack, payload);
    let (bytes, footer) = Packet::encode(&header.kind.payload, &header.info, connection_id);
    (header, bytes, footer)
}

fn prepare_data_transfer2(
    connected: &Host<Connected>,
    payload: Payload,
) -> (SocketAddr, Header<DataTransfer>, Vec<u8>, Footer) {
    let sequence = connected.sequence_tracker;
    let ack = connected.remote_ack_tracker;
    let connection_id = connected.state.connection_id;
    let destination = connected.address;
    let header = Header::data_transfer(sequence, ack, payload);
    let (bytes, footer) = Packet::encode(&header.kind.payload, &header.info, Some(connection_id));
    (destination, header, bytes, footer)
}
