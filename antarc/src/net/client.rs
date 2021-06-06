use std::{
    io::{self, Error},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use log::{debug, error, warn};
use mio::net::UdpSocket;

use super::SendTo;
use crate::{
    events::{FailureEvent, ReceiverEvent, SenderEvent},
    host::{Connected, Disconnected, Generic, Host, HostState, RequestingConnection},
    net::{NetManager, NetworkResource},
    packet::{
        header::Header,
        kind::PacketKind,
        payload::{self, Payload},
        queued::Queued,
        received::Received,
        sequence::Sequence,
        ConnectionId, Encoded, Footer, Packet, Sent, CONNECTION_ACCEPTED, CONNECTION_DENIED,
        CONNECTION_REQUEST, DATA_TRANSFER, HEARTBEAT,
    },
    MTU_LENGTH,
};

/// TODO(alex) 2021-02-26: References for ideas about connection:
/// http://www.tcpipguide.com/free/t_PPPLinkSetupandPhases.htm
///
/// TODO(alex) 2021-03-04: Client and Server are different beasts right now, I'm thinking about
/// ways of allowing some sort of peer-to-peer communication, so a `Client` would have to track
/// connection (`Host<State>`) for multiple other clients. To to this we would need something
/// that looks more like the `Server`, and some way to keep one node of the network as the main
/// server? This idea is not clear yet.
///
/// ADD(alex) [low] 2021-05-27: Piling on this idea, we could just have the user create a network
/// with both a `Client` and a `Server`, this isn't a very good idea, and needs more thought.
#[derive(Debug)]
pub struct Client {
    pub(crate) id_tracker: u64,
    pub(crate) server: Host<Generic>,
    pub(crate) last_sent_time: Duration,
}

impl NetManager<Client> {
    pub fn new_client(address: &SocketAddr) -> Self {
        let client = Client {
            id_tracker: 0,
            server: Host::new_local(),
            last_sent_time: Duration::default(),
        };
        let net_manager = NetManager::new(address, client);
        net_manager
    }

    /// TODO(alex) 2021-02-26: Authentication is something that we can't do here, it's up to the
    /// user, but there must be an API for forcefully dropping a connection, plus banning a host.
    ///
    /// TODO(alex) 2021-05-18: I see two ways of making this function:
    ///
    /// 1. It uses a busy loop that will duplicate a bunch of code from `tick`, the whole receiving
    /// plus send part, this would be ideal with `async`;
    ///
    /// 2. It stays as-is, and we rely on `tick` to handle the connection, but then the user won't
    ///
    /// know much about the connection, we'll be relying on the `retrieve` to return the
    /// `ConnectionId` + the packet payload (which will be done anyway).
    ///
    /// ADD(alex) 2021-05-23: Do the whole connection inside this function, it'll simplify the
    ///
    /// ADD(alex) [mid] 2021-05-28: Either return the events here for the user to treat, or have
    /// a separate function in the API, that passes the errors to the user, I don't like this
    /// approach very much.
    /// `tick` function immensely.
    pub fn connect(&mut self, server_addr: &SocketAddr) -> Result<ConnectionId, String> {
        debug!("Connecting to {:#?}.", server_addr);
        let server = Host::new_generic(server_addr.clone());
        self.kind.server = server;
        self.kind.server.state.state = HostState::RequestingConnection {
            info: RequestingConnection { attempts: 0 },
        };

        let mut time_sent = self.timer.elapsed();

        let id = self.kind.id_tracker;
        let sequence = Sequence::default();
        let ack = 0;
        let payload = Payload::default();
        let payload_length = payload.len() as u16;
        let destination = server_addr.clone();

        let header = Header {
            sequence,
            ack,
            past_acks: 0,
            status_code: CONNECTION_REQUEST,
            payload_length,
        };

        let (bytes, footer) = Packet::encode(&payload, &header, None);
        let mut connection_request = Some(bytes.clone());

        loop {
            self.network
                .poll
                .poll(&mut self.network.events, Some(Duration::from_millis(150)))
                .unwrap();

            for event in self.network.events.iter() {
                if event.token() == NetworkResource::TOKEN {
                    if event.is_readable() {
                        self.network.readable = true;
                    }

                    if event.is_writable() {
                        self.network.writable = true;
                    }
                }
            }

            'receive: while self.network.readable {
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

                        if packet.kind == PacketKind::ConnectionAccepted {
                            debug!("Received ConnectionAccepted!");
                            let connection_id = packet.state.footer.connection_id.unwrap();
                            let info = Connected {
                                connection_id,
                                rtt: Duration::default(),
                                last_sent: 1,
                                time_last_sent: Duration::default(),
                            };
                            self.kind.server.sequence_tracker =
                                unsafe { Sequence::new_unchecked(2) };
                            self.kind.server.state.state = HostState::Connected { info };
                            self.kind.id_tracker += 1;
                            self.kind.last_sent_time = time_sent;
                            self.kind.server.local_ack_tracker = packet.state.header.ack;
                            self.kind.server.remote_ack_tracker =
                                packet.state.header.sequence.get();

                            return Ok(connection_id);
                        } else if packet.kind == PacketKind::ConnectionDenied {
                            debug!("Received ConnectionDenied!");
                            todo!()
                        } else {
                            error!("Received some invalid packet!");
                            panic!("Invalid packet received {:#?}", packet);
                        }
                    }
                    Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                        warn!("Would block on recv {:?}", fail);
                        self.network.readable = false;
                        break 'receive;
                    }
                    Err(fail) => {
                        warn!("Failed recv with {:?}", fail);
                        self.network.readable = false;
                        break 'receive;
                    }
                }
            }

            if self.network.writable == false {
                continue;
            }

            if time_sent + Duration::from_millis(2000) < self.timer.elapsed() {
                debug!("Wait time for connection response expired.");
                connection_request = Some(bytes.clone());
            }

            if let Some(bytes) = connection_request.take() {
                debug!("Attempting to send connection request {:#?}.", bytes);

                match self.network.udp_socket.send_to(&bytes, destination) {
                    Ok(num_sent) => {
                        debug!(
                            "Client sent connection request {:#?} to {:#?}.",
                            header, destination
                        );
                        debug_assert!(num_sent > 0);
                        time_sent = self.timer.elapsed();
                    }
                    Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                        warn!("Would block on send_to {:?}", fail);

                        // TODO(alex) 2021-05-23: Right here, the `bytes` belongs to a
                        // specific Host, so the ownership of this allocation has a
                        // clear owner. When we fail
                        // to send, the encoding is performed again,
                        // even though we could cache it here as a `Packet<Encoded>` and
                        // insert it into the Host.
                        self.event_system
                            .failures
                            .push(FailureEvent::SendConnectionRequest);
                        self.network.writable = false;
                    }
                    Err(fail) => {
                        error!(
                            "Server failed sending connection request {:#?} to {:#?}.",
                            fail, destination
                        );
                        panic!("{}", fail);
                    }
                }
            }
        }
    }

    pub fn enqueue(&mut self, message: Vec<u8>) -> u64 {
        let id = self.kind.id_tracker;
        let destination = self.kind.server.address;
        let state = Queued {
            time: self.timer.elapsed(),
            destination,
        };
        let packet = Packet {
            id,
            state,
            kind: PacketKind::DataTransfer,
        };

        self.event_system
            .sender
            .push(SenderEvent::QueuedDataTransfer { packet });
        self.payload_queue.insert(id, Payload(message));
        self.kind.id_tracker += 1;

        id
    }

    fn heartbeat(&mut self) {}

    pub fn connected(&mut self) {}

    pub fn denied(&mut self) {
        todo!()
    }

    /// TODO(alex) 2021-02-23: Return some indication that the manager received new packets and the
    /// user should call `retrieve`.
    ///
    /// ADD(alex) 2021-02-26: The return can be made even more general, by having an enum of
    /// possibilities, like `HasMessagesToRetrieve`, `ConnectionLost`. The only success cases I can
    /// think of are `HasMessagesToRetrieve` and `NothingToReport`? But the errors are plenty, like
    /// `ReceivingMessageFromBannedHost`, `FailedToSend`, `FailedToReceive`, `FailedToEncode`, ...
    ///
    /// ADD(alex) [mid] 2021-05-28: Either return the events here for the user to treat, or have
    /// a separate function in the API, that passes the errors to the user, I don't like this
    /// approach very much.
    pub fn tick(&mut self) -> Result<usize, String> {
        self.network
            .poll
            .poll(&mut self.network.events, Some(Duration::from_millis(150)))
            .unwrap();

        for event in self.network.events.iter() {
            if event.token() == NetworkResource::TOKEN {
                if event.is_readable() {
                    self.network.readable = true;
                }

                if event.is_writable() {
                    self.network.writable = true;
                }
            }
        }

        while self.network.readable {
            match self.network.udp_socket.recv_from(&mut self.buffer) {
                Ok((num_received, source)) => {
                    debug!("Received new packet {:#?} {:#?}.", num_received, source);
                    debug_assert!(num_received > 0);

                    if source != self.kind.server.address {
                        warn!(
                            "Packet from unknown source, expected {:#?}, got {:#?}.",
                            self.kind.server.address, source
                        );
                        continue;
                    }

                    let (packet, payload) = Packet::decode(
                        self.kind.id_tracker,
                        &self.buffer[..num_received],
                        source,
                        &self.timer,
                    )
                    .unwrap();

                    // TODO(alex) [low] 2021-05-27: Need a mechanism to ack out of order packets,
                    // right now we just don't update the ack trackers if they come from a lower
                    // value than what the host has.
                    self.kind.server.remote_ack_tracker = packet.state.header.sequence.get();
                    self.kind.server.local_ack_tracker = packet.state.header.ack;

                    let local_ack_tracker = self.kind.server.local_ack_tracker;
                    self.kind.server.local_ack_tracker = (packet.state.header.ack
                        > local_ack_tracker)
                        .then(|| packet.state.header.ack)
                        .unwrap_or(self.kind.server.local_ack_tracker);

                    let remote_ack_tracker = self.kind.server.remote_ack_tracker;
                    let remote_sequence = packet.state.header.sequence.get();
                    self.kind.server.remote_ack_tracker = (remote_sequence > remote_ack_tracker)
                        .then(|| packet.state.header.sequence.get())
                        .unwrap_or(self.kind.server.remote_ack_tracker);

                    match packet.kind {
                        PacketKind::ConnectionRequest => {}
                        PacketKind::ConnectionAccepted => {}
                        PacketKind::ConnectionDenied => {}
                        PacketKind::Ack(_) => {}
                        PacketKind::DataTransfer => {
                            // TODO(alex) [mid] 2021-05-26: Insert payload into list of retrievable.
                        }
                        PacketKind::Heartbeat => {}
                    }

                    self.kind.id_tracker += 1;
                }
                Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                    warn!("Would block on recv {:?}", fail);
                    self.network.readable = false;
                    break;
                }
                Err(fail) => {
                    warn!("Failed recv with {:?}", fail);
                    self.network.readable = false;
                    break;
                }
            }
        }
        let threshold = self.kind.last_sent_time + Duration::from_millis(1500);
        let time_expired = threshold < self.timer.elapsed();

        while self.network.writable && self.event_system.sender.len() > 0 {
            for event in self.event_system.sender.drain(..1) {
                match event {
                    SenderEvent::QueuedDataTransfer { packet } => {
                        debug!("Handling QueuedDataTransfer {:#?}.", packet);

                        match &self.kind.server.state.state {
                            HostState::Connected { .. } => (),
                            invalid => {
                                warn!("Client has server in state other than connected!");
                                warn!("Skipping this data transfer!");
                                continue;
                            }
                        }

                        let sequence = self.kind.server.sequence_tracker;
                        let ack = self.kind.server.remote_ack_tracker;
                        let connection_id = self.kind.server.state.state.connection_id();
                        let destination = packet.state.destination;

                        let status_code = From::from(packet.kind);
                        let payload = self.payload_queue.remove(&packet.id).unwrap();
                        let payload_length = payload.len() as u16;

                        let header = Header {
                            sequence,
                            ack,
                            past_acks: 0,
                            status_code,
                            payload_length,
                        };

                        let (bytes, footer) = Packet::encode(&payload, &header, connection_id);

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
                                debug!("Client sent packet {:#?} to {:#?}.", packet, destination);

                                self.kind.server.sequence_tracker =
                                    Sequence::new(sequence.get() + 1).unwrap();
                                self.kind.last_sent_time = self.timer.elapsed();
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
                                    "Client failed sending packet {:#?} to {:#?}.",
                                    fail, destination
                                );

                                break;
                            }
                        }
                    }
                    SenderEvent::QueuedConnectionAccepted { .. } => {}
                    SenderEvent::QueuedConnectionRequest { packet } => {
                        debug!("Handling QueuedConnectionRequest to {:#?}.", packet);

                        let sequence = Sequence::default();
                        let ack = 0;
                        let payload = Payload::default();
                        let payload_length = payload.len() as u16;
                        let destination = packet.state.destination;

                        let header = Header {
                            sequence,
                            ack,
                            past_acks: 0,
                            status_code: CONNECTION_REQUEST,
                            payload_length,
                        };
                        let (bytes, footer) = Packet::encode(&payload, &header, None);

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

                                debug!("Client sent packet {:#?} to {:#?}.", packet, destination);
                                self.kind.last_sent_time = self.timer.elapsed();
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
                    SenderEvent::QueuedHeartbeat { address } => {
                        debug!("Handling QueuedHeartbeat {:#?}.", address);

                        match &self.kind.server.state.state {
                            HostState::Connected { .. } => (),
                            invalid => {
                                warn!("Client has server in state other than connected!");
                                warn!("Skipping this data transfer!");
                                continue;
                            }
                        }

                        let sequence = self.kind.server.sequence_tracker;
                        let ack = self.kind.server.remote_ack_tracker;
                        let connection_id = self.kind.server.state.state.connection_id();
                        let destination = address;

                        let status_code = From::from(HEARTBEAT);
                        let payload = Payload::default();
                        let payload_length = payload.len() as u16;

                        let header = Header {
                            sequence,
                            ack,
                            past_acks: 0,
                            status_code,
                            payload_length,
                        };

                        let (bytes, footer) = Packet::encode(&payload, &header, connection_id);

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
                                debug!("Client sent packet {:#?} to {:#?}.", packet, destination);

                                self.kind.last_sent_time = packet.state.time;
                                self.kind.server.sequence_tracker =
                                    Sequence::new(sequence.get() + 1).unwrap();
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
                                    "Client failed sending packet {:#?} to {:#?}.",
                                    fail, destination
                                );

                                break;
                            }
                        }
                    }
                }
            }
        }

        Ok(self.kind.server.received.len())
    }

    /// NOTE(alex): This is less of a system, and more just a function that the user will call,
    /// part of the public API (exposed via `NetManager` client / server).
    pub fn retrieve(&mut self) -> Vec<(ConnectionId, Vec<u8>)> {
        debug!("Retrieving packets.");
        Vec::with_capacity(4)
    }
}
