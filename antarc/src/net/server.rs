use std::{
    io,
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
    time::{Duration, Instant},
};

use log::{debug, error, warn};

use super::SendTo;
use crate::{
    events::{CommonEvent, ReceiverEvent, SenderEvent},
    host::{AwaitingConnectionAck, Connected, Disconnected, Host, HostState, RequestingConnection},
    net::{NetManager, NetworkResource},
    packet::{
        header::Header, ConnectionId, Encoded, Footer, Packet, PacketKind, Payload, Queued,
        Received, Sent, Sequence, CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST,
        DATA_TRANSFER, HEARTBEAT,
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
            let packet = Packet {
                id,
                state,
                kind: PacketKind::DataTransfer,
            };

            // self.user_queue.push(packet);
            self.event_system
                .sender
                .push(SenderEvent::QueuedDataTransfer { packet });
        }

        self.payload_queue.insert(id, Payload(message));
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
                    let (packet, payload) = Packet::decode(
                        self.kind.id_tracker,
                        &self.buffer[..num_received],
                        source,
                        &self.timer,
                    )
                    .unwrap();
                    debug!("Received new packet {:#?} {:#?}.", packet, source);

                    self.event_system.receiver.push(ReceiverEvent::AckRemote {
                        header: packet.state.header.clone(),
                    });

                    match packet.kind {
                        PacketKind::ConnectionRequest => {
                            self.event_system
                                .receiver
                                .push(ReceiverEvent::ConnectionRequest { packet });
                        }
                        PacketKind::Ack(_) => {}
                        PacketKind::DataTransfer => {
                            self.event_system
                                .receiver
                                .push(ReceiverEvent::DataTransfer { packet, payload });
                        }
                        PacketKind::Heartbeat => {
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
                    self.network.readable = false;
                }
            }
        }

        let has_host = self.kind.requesting_connection.len() > 0
            || self.kind.awaiting_connection_ack.len() > 0
            || self.kind.connected.len() > 0;
        let has_queued = self.event_system.sender.len() > 0;

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

        while self.network.writable && has_host && self.event_system.sender.len() > 0 {
            debug_assert!(self.event_system.sender.len() > 0);

            for event in self.event_system.sender.drain(..1) {
                match event {
                    SenderEvent::QueuedDataTransfer { packet } => {
                        debug!("Handling QueuedDataTransfer {:#?}.", packet);

                        let payload = self.payload_queue.get(&packet.id).unwrap();
                        let payload_length = payload.len() as u16;

                        let connected = self
                            .kind
                            .connected
                            .iter_mut()
                            .find(|host| host.address == packet.state.destination)
                            .unwrap();

                        let sequence = connected.sequence_tracker;
                        let ack = connected.remote_ack_tracker;
                        let connection_id = connected.state.connection_id;
                        let destination = connected.address;
                        let header = Header {
                            sequence,
                            ack,
                            past_acks: 0,
                            status_code: DATA_TRANSFER,
                            payload_length,
                        };
                        let (bytes, footer) =
                            match Packet::encode(&payload, &header, Some(connection_id)) {
                                Ok(encoded) => encoded,
                                Err(fail) => {
                                    error!("Failed encoding packet {:#?}.", fail);
                                    break;
                                }
                            };

                        match self.network.udp_socket.send_to(&bytes, destination) {
                            Ok(num_sent) => {
                                debug_assert!(num_sent > 0);

                                let sent = Sent {
                                    header,
                                    footer,
                                    destination,
                                    time: self.timer.elapsed(),
                                };
                                let packet = Packet {
                                    id: packet.id,
                                    state: sent,
                                    kind: packet.kind,
                                };
                                debug!("Server sent packet {:#?} to {:#?}.", packet, destination);

                                let sent_event = CommonEvent::SentPacket { packet };
                                connected.sequence_tracker =
                                    Sequence::new(connected.sequence_tracker.get() + 1).unwrap();
                            }
                            Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                                warn!("Would block on send_to {:?}", fail);

                                // TODO(alex) [low] 2021-05-23: Right here, the `bytes` belongs to a
                                // specific Host, so the ownership of this allocation has a
                                // clear owner. When we fail
                                // to send, the encoding is performed again,
                                // even though we could cache it here as a `Packet<Encoded>` and
                                // insert it into the Host.
                                self.network.writable = false;

                                break;
                            }
                            Err(fail) => {
                                error!(
                                    "Server failed sending packet {:#?} to {:#?}.",
                                    fail, destination
                                );
                                break;
                            }
                        }
                    }
                    SenderEvent::QueuedConnectionAccepted { packet } => {
                        debug!("Handling QueuedConnectionAccepted {:#?}.", packet);

                        let payload = Payload::default();
                        let payload_length = payload.len() as u16;

                        let requesting_connection = self
                            .kind
                            .requesting_connection
                            .drain_filter(|host| host.address == packet.state.destination)
                            .next()
                            .unwrap();

                        let sequence = Sequence::default();
                        let ack = 1;
                        let connection_id = self.kind.connection_id_tracker;
                        let destination = requesting_connection.address;
                        let header = Header {
                            sequence,
                            ack,
                            past_acks: 0,
                            status_code: CONNECTION_ACCEPTED,
                            payload_length,
                        };
                        let (bytes, footer) =
                            match Packet::encode(&payload, &header, Some(connection_id)) {
                                Ok(encoded) => encoded,
                                Err(fail) => {
                                    error!("Failed encoding packet {:#?}.", fail);
                                    break;
                                }
                            };

                        match self.network.udp_socket.send_to(&bytes, destination) {
                            Ok(num_sent) => {
                                debug_assert!(num_sent > 0);

                                let sent = Sent {
                                    header,
                                    footer,
                                    destination,
                                    time: self.timer.elapsed(),
                                };
                                let packet = Packet {
                                    id: packet.id,
                                    state: sent,
                                    kind: packet.kind,
                                };
                                debug!("Server sent packet {:#?} to {:#?}.", packet, destination);
                                let sent_event = CommonEvent::SentPacket { packet };

                                let sequence_tracker = Sequence::new(sequence.get() + 1).unwrap();
                                let state = AwaitingConnectionAck {
                                    attempts: 0,
                                    connection_id: self.kind.connection_id_tracker,
                                    last_sent: 1,
                                };
                                let host = Host {
                                    sequence_tracker,
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
                            Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                                warn!("Would block on send_to {:?}", fail);

                                // TODO(alex) 2021-05-23: Right here, the `bytes` belongs to a
                                // specific Host, so the ownership of this allocation has a
                                // clear owner. When we fail
                                // to send, the encoding is performed again,
                                // even though we could cache it here as a `Packet<Encoded>` and
                                // insert it into the Host.
                                self.network.writable = false;
                                self.kind.requesting_connection.push(requesting_connection);

                                break;
                            }
                            Err(fail) => {
                                error!(
                                    "Server failed sending packet {:#?} to {:#?}.",
                                    fail, destination
                                );
                                self.kind.requesting_connection.push(requesting_connection);
                                break;
                            }
                        }
                    }
                    SenderEvent::QueuedConnectionRequest { packet } => {
                        error!("Server cannot handle SendConnectionRequest!");
                        unreachable!();
                    }
                    SenderEvent::QueuedHeartbeat { address } => {
                        debug!("Handling QueuedHeartbeat to {:#?}.", address);

                        let payload = Payload::default();
                        let payload_length = payload.len() as u16;

                        let connected = self
                            .kind
                            .connected
                            .iter_mut()
                            .find(|host| host.address == address)
                            .unwrap();

                        let sequence = connected.sequence_tracker;
                        let ack = connected.remote_ack_tracker;
                        let connection_id = connected.state.connection_id;
                        let destination = connected.address;
                        let header = Header {
                            sequence,
                            ack,
                            past_acks: 0,
                            status_code: HEARTBEAT,
                            payload_length,
                        };
                        let (bytes, footer) =
                            match Packet::encode(&payload, &header, Some(connection_id)) {
                                Ok(encoded) => encoded,
                                Err(fail) => {
                                    error!("Failed encoding packet {:#?}.", fail);
                                    break;
                                }
                            };

                        match self.network.udp_socket.send_to(&bytes, destination) {
                            Ok(num_sent) => {
                                debug_assert!(num_sent > 0);

                                let sent = Sent {
                                    header,
                                    footer,
                                    destination,
                                    time: self.timer.elapsed(),
                                };
                                let id = self.kind.id_tracker;
                                let packet = Packet {
                                    id,
                                    state: sent,
                                    kind: PacketKind::Heartbeat,
                                };
                                debug!("Server sent packet {:#?} to {:#?}.", packet, destination);

                                connected.sequence_tracker =
                                    Sequence::new(connected.sequence_tracker.get() + 1).unwrap();
                                self.kind.id_tracker += 1;
                            }
                            Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                                warn!("Would block on send_to {:?}", fail);

                                // TODO(alex) [low] 2021-05-23: Right here, the `bytes` belongs to a
                                // specific Host, so the ownership of this allocation has a
                                // clear owner. When we fail
                                // to send, the encoding is performed again,
                                // even though we could cache it here as a `Packet<Encoded>` and
                                // insert it into the Host.
                                self.network.writable = false;

                                break;
                            }
                            Err(fail) => {
                                error!(
                                    "Server failed sending packet {:#?} to {:#?}.",
                                    fail, destination
                                );
                                break;
                            }
                        }
                    }
                }
            }
        }

        // TODO(alex) [mid] 2021-05-26: This whole common event thing could go away, we know the
        // kind of packet we're sending at send time, so no need to match, whatever needs to happen
        // after send could be done inline (this will generate duplicated code, and I'll probably
        // end up coming back to this "post-effect" event handler anyway, but let's get a clearer
        // view of the design first).
        for event in self.event_system.common.drain(..) {
            match event {
                CommonEvent::SentPacket { packet: sent } => {
                    debug!("Handling SentPacket for {:#?}", sent);

                    match sent.kind {
                        PacketKind::ConnectionRequest => {
                            error!("Server cannot have sent a connection request {:#?}.", sent);
                            unreachable!();
                        }
                        PacketKind::ConnectionAccepted => {
                            debug!("Server sent ConnectionAccepted, no payload to remove.");
                        }
                        PacketKind::ConnectionDenied => {
                            debug!("Server sent ConnectionDenied.");
                        }
                        PacketKind::Ack(_) => {}
                        PacketKind::DataTransfer => {
                            debug!("Server sent DataTransfer.");

                            if self
                                .kind
                                .connected
                                .iter()
                                .all(|host| host.state.last_sent >= sent.id)
                            {
                                let removed_payload = self.payload_queue.remove(&sent.id);
                                debug!("Removed {:#?} from queue.", removed_payload);
                            }
                        }
                        PacketKind::Heartbeat => {}
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
                    let mut received_list = Vec::with_capacity(128);

                    // TODO(alex) [low] 2021-05-26: Always creating a new host here, as I've dropped
                    // the concept of a `Disconnected` host, this could be revisited later when the
                    // design is clearer.
                    let ack_tracker = packet.state.header.sequence.get();
                    let state = RequestingConnection { attempts: 0 };
                    received_list.append(&mut vec![packet]);
                    let host = Host {
                        sequence_tracker: Sequence::default(),
                        remote_ack_tracker: ack_tracker,
                        local_ack_tracker: 0,
                        address: source,
                        received: received_list,
                        state,
                    };

                    let id = self.kind.id_tracker;
                    let state = Queued {
                        time: self.timer.elapsed(),
                        destination: host.address,
                    };
                    let kind = PacketKind::ConnectionAccepted;
                    let packet = Packet { id, state, kind };
                    let connection_accepted = SenderEvent::QueuedConnectionAccepted { packet };

                    self.kind.requesting_connection.push(host);
                    self.event_system.sender.push(connection_accepted);
                    self.kind.id_tracker += 1;
                }
                ReceiverEvent::AckRemote { header } => {}
                ReceiverEvent::DataTransfer { packet, payload } => {
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
                            h.address == packet.state.source && packet.state.header.ack == 1
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

                        host.local_ack_tracker = (packet.state.header.ack > host.local_ack_tracker)
                            .then(|| packet.state.header.ack)
                            .unwrap_or(host.local_ack_tracker);

                        let remote_sequence = packet.state.header.sequence.get();
                        host.remote_ack_tracker = (remote_sequence > host.remote_ack_tracker)
                            .then(|| packet.state.header.sequence.get())
                            .unwrap_or(host.remote_ack_tracker);

                        host.received.push(packet);

                        if self.retrievable.contains_key(&host.state.connection_id) {
                            self.retrievable
                                .get_mut(&host.state.connection_id)
                                .unwrap()
                                .push(payload.0);
                        } else {
                            self.retrievable
                                .insert(host.state.connection_id, vec![payload.0]);
                        }
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
                            "Host is requesting connection , invalid for heartbeat {:#?}!",
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
                            h.address == packet.state.source && packet.state.header.ack == 1
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

                        host.local_ack_tracker = (packet.state.header.ack > host.local_ack_tracker)
                            .then(|| packet.state.header.ack)
                            .unwrap_or(host.local_ack_tracker);

                        let remote_sequence = packet.state.header.sequence.get();
                        host.remote_ack_tracker = (remote_sequence > host.remote_ack_tracker)
                            .then(|| packet.state.header.sequence.get())
                            .unwrap_or(host.remote_ack_tracker);

                        host.received.push(packet);
                    }
                }
            }
        }

        Ok(self.retrievable_count)
    }

    // TODO(alex) [low] 2021-05-26: Handling events like this won't work, as we get a double borrow
    // from `DrainIter` in `tick`. This isn't possible because when we borrow `self` as mutable to
    // drain the list of events, rust wouldn't be able to tell what changes we could do here. It
    // works perfectly fine when the code is inlined (pasted) in the drain loop, as rust can tell
    // we're not making changes to the very thing we're iterating over.
    //
    // Refactor this in the future, when we know what this function does exactly, then we can avoid
    // passing a full reference to `self`.
    fn _received_connection_request(&self, packet: Packet<Received>) {
        debug!("Handling received connecion request for {:#?}", packet);

        let source = packet.state.source;
        let ack_tracker = packet.state.header.sequence.get();

        // TODO(alex) [low] 2021-05-25: Is there a way to better handle this case?
        if self
            .kind
            .requesting_connection
            .iter()
            .any(|h| h.address == source)
            || self.kind.connected.iter().any(|h| h.address == source)
        {
            error!("Host is already in another state to receive this packet!");
            unreachable!();
        }

        let info = RequestingConnection { attempts: 0 };
        let state = HostState::RequestingConnection { info };

        debug!("Unknown address, creating new host.");
        let mut received_list = Vec::with_capacity(128);
        received_list.append(&mut vec![packet]);

        let host = Host {
            sequence_tracker: Sequence::default(),
            remote_ack_tracker: ack_tracker,
            local_ack_tracker: 0,
            address: source,
            received: received_list,
            state,
        };
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
