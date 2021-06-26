use std::{
    io::{self, Error},
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use antarc_protocol::{
    events::SenderEvent,
    hosts::Host,
    packets::{
        header::Header, payload::Payload, scheduled::Scheduled, ConnectionId, Packet, Sent,
        CONNECTION_ACCEPTED, CONNECTION_DENIED,
    },
    Client, Protocol,
};
use log::{debug, error, warn};
use mio::net::UdpSocket;

use super::SendTo;
use crate::{
    net::{NetManager, NetworkResource},
    send, MTU_LENGTH,
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
impl NetManager<Client> {
    pub fn new_client(address: &SocketAddr) -> Self {
        let protocol = Protocol::new_client();
        let net_manager = NetManager::new(address, protocol);
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
    /// `tick` function immensely.
    ///
    /// ADD(alex) [mid] 2021-05-28: Either return the events here for the user to treat, or have
    /// a separate function in the API, that passes the errors to the user, I don't like this
    /// approach very much.
    pub fn connect(&mut self, server_addr: &SocketAddr) -> Result<ConnectionId, String> {
        debug!("Connecting to {:#?}.", server_addr);
        let server = Host::starting_connection_request(server_addr.clone());
        self.connection.requesting_connection.insert(0, server);

        let mut time_sent = self.protocol.timer.elapsed();

        let payload = Payload::default();
        let destination = server_addr.clone();
        let header = Header::connection_request();

        let (bytes, _) = Packet::encode(&payload, &header.info, None);
        let mut connection_request = Some(bytes.clone());

        // TODO(alex) [vhigh] 2021-06-14: Make this work.
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

                        let (packet, _) = Packet::decode(
                            self.connection.packet_id_tracker,
                            &self.buffer[..num_received],
                            source,
                            &self.protocol.timer,
                        )
                        .unwrap();
                        debug!("Received new packet {:#?} {:#?}.", packet, source);

                        if packet.state.header.info.status_code == CONNECTION_ACCEPTED {
                            debug!("Received ConnectionAccepted!");
                            let server = self.connection.awaiting_connection_ack.remove(0);
                            let connection_id = packet.state.footer.connection_id.unwrap();
                            let mut connected = server.connection_accepted(connection_id);
                            let connection_id = packet.state.footer.connection_id.unwrap();
                            self.connection.packet_id_tracker += 1;
                            self.protocol.kind.last_sent_time = time_sent;
                            connected.local_ack_tracker = packet.state.header.info.ack;
                            connected.remote_ack_tracker = packet.state.header.info.sequence.get();

                            self.connection.connected.insert(0, connected);

                            return Ok(connection_id);
                        } else if packet.state.header.info.status_code == CONNECTION_DENIED {
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

            if time_sent + Duration::from_millis(2000) < self.protocol.timer.elapsed() {
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
                        time_sent = self.protocol.timer.elapsed();

                        // TODO(alex) [high] 2021-06-09: This breaks the whole client-side, we need
                        // a way to convert `RequestingConnection->???` into something that is
                        // awaiting a connection accepted/denied, the current struct used by the
                        // server is `AwaitingConnectionAck`, but cannot be used here, as it
                        // requires a `ConnectionId`. It's ok for the server, as it's generated
                        // there, but the client can only wait for it.
                        //
                        // Could we just nuke this field from `AwaitingConnectionAck`? If a new
                        // state is required (`AwaitingConnectionResponse` for example) that doesn'T
                        // have this field, then `NetManager::Connection` will be different for
                        // `Server` and `Client`.
                        let server = self.connection.requesting_connection.remove(0);
                        let awaiting_connection = server.await_connection();
                        self.connection
                            .awaiting_connection_ack
                            .push(awaiting_connection);
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

    pub fn enschedule(&mut self, message: Vec<u8>) -> u64 {
        let id = self.connection.packet_id_tracker;
        let destination = self.connection.connected.get(0).unwrap().address;
        let state = Scheduled {
            time: self.protocol.timer.elapsed(),
            destination,
        };
        let packet = Packet { id, state };

        self.protocol
            .event_system
            .sender
            .push(SenderEvent::ScheduledDataTransfer {
                packet,
                payload: Payload(message),
            });
        self.connection.packet_id_tracker += 1;

        id
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
    ///
    /// ADD(alex) [mid] 2021-06-06: Use `thiserror` + `?` operator. Requires having a proper
    /// `AntarcError` enum.
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

                    if source != self.connection.connected.get(0).unwrap().address {
                        warn!(
                            "Packet from unknown source, expected {:#?}, got {:#?}.",
                            self.connection.connected.get(0).unwrap().address,
                            source
                        );
                        continue;
                    }

                    let (packet, payload) = Packet::decode(
                        self.connection.packet_id_tracker,
                        &self.buffer[..num_received],
                        source,
                        &self.protocol.timer,
                    )
                    .unwrap();

                    // TODO(alex) [low] 2021-05-27: Need a mechanism to ack out of order packets,
                    // right now we just don't update the ack trackers if they come from a lower
                    // value than what the host has.
                    //
                    // TODO(alex) [vhigh] 2021-06-13: Check server ln-344 to see how it's handling
                    // this mountain of code, it involves finally handling the `ReceiverEvent`s for
                    // client.
                    self.connection
                        .connected
                        .get_mut(0)
                        .unwrap()
                        .remote_ack_tracker = packet.state.header.info.sequence.get();
                    self.connection
                        .connected
                        .get_mut(0)
                        .unwrap()
                        .local_ack_tracker = packet.state.header.info.ack;

                    let local_ack_tracker =
                        self.connection.connected.get(0).unwrap().local_ack_tracker;
                    self.connection
                        .connected
                        .get_mut(0)
                        .unwrap()
                        .local_ack_tracker = (packet.state.header.info.ack > local_ack_tracker)
                        .then(|| packet.state.header.info.ack)
                        .unwrap_or(self.connection.connected.get(0).unwrap().local_ack_tracker);

                    let remote_ack_tracker =
                        self.connection.connected.get(0).unwrap().remote_ack_tracker;
                    let remote_sequence = packet.state.header.info.sequence.get();
                    self.connection
                        .connected
                        .get_mut(0)
                        .unwrap()
                        .remote_ack_tracker = (remote_sequence > remote_ack_tracker)
                        .then(|| packet.state.header.info.sequence.get())
                        .unwrap_or(self.connection.connected.get(0).unwrap().remote_ack_tracker);

                    match packet.state.header.info.status_code {
                        CONNECTION_REQUEST => {
                            unreachable!()
                        }
                        DATA_TRANSFER => {
                            // TODO(alex) [high] 2021-05-26: Insert payload into list of
                            // retrievable.
                            //
                            // ADD(alex) [high] 2021-06-06: Handle these kinds of events!
                        }
                        HEARTBEAT => {
                            todo!()
                        }
                        invalid => {
                            panic!("Status code is {:#?}.", invalid);
                        }
                    }

                    self.connection.packet_id_tracker += 1;
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

        // TODO(alex) [high] 2021-06-06: Finally de-duplicate this code.
        //
        // ADD(alex) [high] 2021-06-08: Trimmed down some of the duplicated code, but this function
        // is still quite big and repetitive.
        while self.network.writable && self.protocol.event_system.sender.len() > 0 {
            for event in self.protocol.event_system.sender.drain(..1) {
                match event {
                    SenderEvent::ScheduledDataTransfer { packet, payload } => {
                        debug!("Handling ScheduledDataTransfer {:#?}.", packet);

                        let destination = packet.state.destination;
                        let connected = self.connection.connected.get_mut(0).unwrap();
                        let (header, bytes, footer) = connected.prepare_data_transfer(&payload);

                        send!(
                            self,
                            connected,
                            bytes,
                            OnError -> todo!()
                        );

                        let sent = packet.to_sent(
                            header,
                            footer,
                            self.protocol.timer.elapsed(),
                            destination,
                        );
                        connected.state.sent_data_transfers.push(sent);
                    }
                    SenderEvent::ScheduledConnectionAccepted { .. } => {
                        unreachable!()
                    }
                    SenderEvent::ScheduledConnectionRequest { packet } => {
                        debug!("Handling ScheduledConnectionRequest to {:#?}.", packet);

                        let payload = Payload::default();
                        let destination = packet.state.destination;
                        let header = Header::connection_request();

                        let (bytes, footer) = Packet::encode(&payload, &header.info, None);

                        match self.network.udp_socket.send_to(&bytes, destination) {
                            Ok(num_sent) => {
                                debug_assert!(num_sent > 0);

                                let sent = Sent {
                                    header,
                                    footer,
                                    destination,
                                    time: self.protocol.timer.elapsed(),
                                };
                                let packet = Packet {
                                    id: packet.id,
                                    state: sent,
                                };

                                debug!("Client sent packet {:#?} to {:#?}.", packet, destination);
                                self.protocol.kind.last_sent_time = self.protocol.timer.elapsed();
                            }
                            Err(fail) => {
                                if fail.kind() == io::ErrorKind::WouldBlock {
                                    warn!("Would block on send_to {:?}", fail);
                                    self.network.writable = false;
                                }

                                // NOTE(alex): Cannot use `bytes` here (or in any failure event), as
                                // it could end up being a duplicated packet, sequence and ack are
                                // only incremented when send is successful.
                                todo!();
                                break;
                            }
                        }
                    }
                    SenderEvent::ScheduledHeartbeat { address } => {
                        debug!("Handling ScheduledHeartbeat {:#?}.", address);

                        let connected = self.connection.connected.get_mut(0).unwrap();
                        let (header, bytes, footer) = connected.prepare_heartbeat();

                        send!(self, connected, bytes,
                            OnError -> todo!());

                        let sent = Packet::sent_heartbeat(
                            self.connection.packet_id_tracker,
                            header,
                            footer,
                            self.protocol.timer.elapsed(),
                            address,
                        );
                        connected.state.sent_heartbeats.push(sent);
                    }
                }
            }
        }

        todo!()
    }

    /// NOTE(alex): This is less of a system, and more just a function that the user will call,
    /// part of the public API (exposed via `NetManager` client / server).
    pub fn retrieve(&mut self) -> Vec<(ConnectionId, Vec<u8>)> {
        debug!("Retrieving packets.");
        Vec::with_capacity(4)
    }
}
