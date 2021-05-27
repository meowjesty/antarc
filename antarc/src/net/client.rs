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
    events::{CommonEvent, ConnectionEvent, EventKind, ReceiverEvent, SenderEvent},
    host::{Connected, Disconnected, Generic, Host, HostState, RequestingConnection},
    net::{NetManager, NetworkResource},
    packet::{
        header::Header, ConnectionId, Encoded, Footer, Packet, PacketKind, Payload, Queued,
        Received, Sent, Sequence, CONNECTION_ACCEPTED, CONNECTION_DENIED, CONNECTION_REQUEST,
        DATA_TRANSFER, HEARTBEAT,
    },
    MTU_LENGTH,
};

/// TODO(alex) 2021-05-01: Consider adding a `DebugName` component for every entity, such as when
/// doing `world.spawn((format!("Packet {:?}", header), components...))`.

/// TODO(alex) 2021-02-26: References for ideas about connection:
/// http://www.tcpipguide.com/free/t_PPPLinkSetupandPhases.htm
///
/// ADD(alex) 2021-02-26: We need a `Disconnecting` state (link termination phase)?
///
/// TODO(alex) 2021-03-04: Client and Server are different beasts right now, I'm thinking about
/// ways of allowing some sort of peer-to-peer communication, so a `Client` would have to track
/// connection (`Host<State>`) for multiple other clients. To to this we would need something
/// that looks more like the `Server`, and some way to keep one node of the network as the main
/// server? This idea is not clear yet.
#[derive(Debug)]
pub struct Client {
    pub(crate) id_tracker: u64,
    pub(crate) server: Host<Generic>,
    pub(crate) connection_events: Vec<ConnectionEvent>,
    pub(crate) last_sent_time: Duration,
}

impl NetManager<Client> {
    pub fn new_client(address: &SocketAddr) -> Self {
        let client = Client {
            id_tracker: 0,
            server: Host::new_local(),
            connection_events: Vec::with_capacity(8),
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
    /// know much about the connection, we'll be relying on the `retrieve` to return the
    /// `ConnectionId` + the packet payload (which will be done anyway).
    ///
    /// ADD(alex) 2021-05-23: Do the whole connection inside this function, it'll simplify the
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

        let (bytes, footer) = match Packet::encode(&payload, &header, None) {
            Ok(encoded) => encoded,
            Err(fail) => {
                error!("Failed encoding packet {:#?}.", fail);
                panic!("{}", fail);
            }
        };

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
                        warn!("Would block on recv_from {:?}", fail);
                        self.network.readable = false;
                        break 'receive;
                    }
                    Err(fail) => {
                        warn!("Failed recv_from with {:?}", fail);
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
    /// ADD(alex) 2021-02-26: The return can be made even more general, by having an enum of
    /// possibilities, like `HasMessagesToRetrieve`, `ConnectionLost`. The only success cases I can
    /// think of are `HasMessagesToRetrieve` and `NothingToReport`? But the errors are plenty, like
    /// `ReceivingMessageFromBannedHost`, `FailedToSend`, `FailedToReceive`, `FailedToEncode`, ...
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
                    let (packet, payload) = Packet::decode(
                        self.kind.id_tracker,
                        &self.buffer[..num_received],
                        source,
                        &self.timer,
                    )
                    .unwrap();

                    self.event_system.receiver.push(ReceiverEvent::AckRemote {
                        header: packet.state.header.clone(),
                    });

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
                    warn!("Would block on recv_from {:?}", fail);
                    self.network.readable = false;
                    break;
                }
                Err(fail) => {
                    warn!("Failed recv_from with {:?}", fail);
                    self.network.readable = false;
                    break;
                }
            }
        }
        let threshold = self.kind.last_sent_time + Duration::from_millis(1500);
        let time_expired = threshold < self.timer.elapsed();
        let has_packet_to_send = self.event_system.sender.len() > 0;
        if has_packet_to_send == false && time_expired {
            debug!(
                "Too much time has passed since we sent a packet {:#?}!",
                threshold
            );
            self.event_system.sender.push(SenderEvent::QueuedHeartbeat {
                address: self.kind.server.address,
            })
        }
        while self.network.writable && has_packet_to_send {
            if self.event_system.sender.len() == 0 {
                break;
            }

            // TODO(alex) [high] 2021-05-27: Can't proceed here, we won't have a packet, sometimes.
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
                        let payload = self.payload_queue.get(&packet.id).unwrap();
                        let payload_length = payload.len() as u16;

                        let header = Header {
                            sequence,
                            ack,
                            past_acks: 0,
                            status_code,
                            payload_length,
                        };

                        let (bytes, footer) = match Packet::encode(&payload, &header, connection_id)
                        {
                            Ok(success) => success,
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
                                // TODO(alex) [high] 2021-05-27: Find why the client always send
                                // `ack: 1`.
                                debug!("Client sent packet {:#?} to {:#?}.", packet, destination);

                                let sent_event = CommonEvent::SentPacket { packet };
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
                    SenderEvent::QueuedConnectionAccepted { packet } => {}
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
                        let (bytes, footer) = match Packet::encode(&payload, &header, None) {
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
                                // TODO(alex) [high] 2021-05-27: Find why the client always send
                                // `ack: 1`.
                                debug!("Client sent packet {:#?} to {:#?}.", packet, destination);
                                let sent_event = CommonEvent::SentPacket { packet };
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

                        let (bytes, footer) = match Packet::encode(&payload, &header, connection_id)
                        {
                            Ok(success) => success,
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
                                debug!("Client sent packet {:#?} to {:#?}.", packet, destination);

                                self.kind.last_sent_time = packet.state.time;
                                let sent_event = CommonEvent::SentPacket { packet };
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

        for event in self.event_system.common.drain(..) {
            match event {
                CommonEvent::SentPacket { packet } => {
                    debug!("Handling SentPacket for {:#?}", packet);

                    match packet.kind {
                        PacketKind::ConnectionRequest => {}
                        PacketKind::ConnectionAccepted => {}
                        PacketKind::ConnectionDenied => {}
                        PacketKind::Ack(_) => {}
                        PacketKind::DataTransfer => {
                            let removed_payload = self.payload_queue.remove(&packet.id).unwrap();
                            debug!("Removed {:#?} from queue.", removed_payload);
                        }
                        PacketKind::Heartbeat => {}
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
