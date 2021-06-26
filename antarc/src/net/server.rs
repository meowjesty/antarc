use std::{
    io,
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
    time::{Duration, Instant},
    vec::Drain,
};

use antarc_protocol::{
    events::SenderEvent,
    packets::{payload::Payload, raw::RawPacket, scheduled::Scheduled, ConnectionId, Packet},
    PacketId, Protocol, Server,
};
use log::{debug, error, warn};

use super::SendTo;
use crate::net::{NetManager, NetworkResource};

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
                // $self.event_system.failures.push($failed);

                break;
            }
        }
    }};
}

impl NetManager<Server> {
    pub fn new_server(address: &SocketAddr) -> Self {
        let protocol = Protocol::new_server();
        let net_manager = NetManager::new(address, protocol);
        net_manager
    }

    // TODO(alex) 2021-05-23: Allow the user to specify the destination of these messages, we then
    // check if they're in the `connected` list, and schedule the packets as normal.
    pub fn schedule(&mut self, message: Vec<u8>) -> PacketId {
        let id = self.connection.packet_id_tracker;

        for host in self.connection.connected.iter() {
            let state = Scheduled {
                time: self.protocol.timer.elapsed(),
                destination: host.address,
            };
            let packet = Packet { id, state };

            self.protocol
                .event_system
                .sender
                .push(SenderEvent::ScheduledDataTransfer {
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
                    let raw_packet = RawPacket::new(source, self.buffer[..num_received].to_vec());
                    self.protocol.on_received(raw_packet);
                }
                Err(fail) if fail.kind() == io::ErrorKind::WouldBlock => {
                    warn!("Would block on recv_from {:?}", fail);
                    self.network.readable = false;
                }
                Err(fail) => {}
            }
        }

        // TODO(alex) [mid] 2021-05-27: Send a heartbeat, look at the connected hosts only.
        for host in self.connection.connected.iter() {
            let threshold = host.state.latest_sent_time + Duration::from_millis(1500);
            let time_expired = threshold < self.protocol.timer.elapsed();
            if time_expired {
                debug!(
                    "Time expired, sending host a heartbeat {:#?}.",
                    host.address
                );
                self.protocol
                    .event_system
                    .sender
                    .push(SenderEvent::ScheduledHeartbeat {
                        address: host.address,
                    });
            }
        }

        // TODO(alex) [high] 2021-06-06: Finally de-duplicate this code.
        while self.network.writable
            && self.protocol.event_system.sender.is_empty() == false
            && self.connection.is_empty() == false
        {
            debug_assert!(self.protocol.event_system.sender.len() > 0);

            for event in self.protocol.event_system.sender.drain(..1) {
                match event {
                    SenderEvent::ScheduledDataTransfer { packet, payload } => {
                        debug!("Handling ScheduledDataTransfer {:#?}.", packet);

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

                        let sent = packet.to_sent(
                            header,
                            footer,
                            self.protocol.timer.elapsed(),
                            destination,
                        );
                        connected.state.sent_data_transfers.push(sent);
                    }
                    SenderEvent::ScheduledConnectionAccepted { packet } => {
                        debug!("Handling ScheduledConnectionAccepted {:#?}.", packet);

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

                        let connection_id = self.protocol.kind.connection_id_tracker;
                        let (_, bytes, _) =
                            requesting_connection.prepare_connection_accepted(connection_id);

                        send!(self, requesting_connection, bytes,
                            OnError -> FailureEvent::SendConnectionAccepted { packet });

                        let requesting_connection =
                            self.connection.requesting_connection.remove(index);
                        let host = requesting_connection.await_connection();
                        self.connection.awaiting_connection_ack.push(host);

                        self.protocol.kind.connection_id_tracker =
                            NonZeroU16::new(self.protocol.kind.connection_id_tracker.get() + 1)
                                .unwrap();
                    }
                    SenderEvent::ScheduledConnectionRequest { .. } => {
                        error!("Server cannot handle SendConnectionRequest!");
                        unreachable!();
                    }
                    SenderEvent::ScheduledHeartbeat { address } => {
                        debug!("Handling ScheduledHeartbeat to {:#?}.", address);

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

                        // TODO(alex) [mid] 2021-06-13: Don't think I need to keep a list of every
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
                            self.protocol.timer.elapsed(),
                            address,
                        );
                        connected.state.sent_heartbeats.push(sent);
                    }
                }
            }
        }

        // TODO(alex) [low] 2021-05-26: Check `fn _received_connection_request` notes.

        Ok(self.protocol.retrievable.len())
    }

    // TODO(alex) [vlow] 2021-06-13: It might be a good idea to return a bit more information than
    // just `ConnectionId`, such as a sequence, or maybe time received, so that the user may know
    // which payload is the "freshest".
    pub fn retrieve(&mut self) -> Drain<(ConnectionId, Vec<u8>)> {
        debug!("Retrieve for server.");
        self.protocol.retrieve()
    }
}
