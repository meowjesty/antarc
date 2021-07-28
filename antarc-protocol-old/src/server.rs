#![feature(more_qualified_paths)]

use std::{convert::TryInto, time::Duration};

use log::{debug, warn};

use crate::{
    events::{AntarcEvent, ProtocolError, ReceiverEvent},
    hosts::Host,
    packets::{partial::PartialPacket, raw::RawPacket, received::Received, ConnectionId, Packet},
    EventSystem, Protocol,
};

#[derive(Debug)]
pub struct Server {
    pub last_antarc_schedule_check: Duration,
    pub connection_id_tracker: ConnectionId,
}

impl Protocol<Server> {
    pub fn new_server() -> Self {
        todo!()
    }

    pub fn received(&mut self, raw_packet: RawPacket) {
        match raw_packet.decode(self.connection_system.packet_id_tracker) {
            Ok(partial_packet) => self.receiver_pipe.push(partial_packet),
            Err(fail) => self.event_pipe.api.push(AntarcEvent::Fail(fail)),
        }
    }

    pub fn received_connection_request(
        event_system: &mut EventSystem,
        partial_packet: PartialPacket,
    ) {
        match partial_packet.try_into() {
            Ok(packet) => event_system
                .receiver
                .push(ReceiverEvent::ConnectionRequest { packet }),
            Err(fail) => event_system.api.push(AntarcEvent::Fail(fail)),
        }
    }

    pub fn received_connection_accepted(
        event_system: &mut EventSystem,
        partial_packet: PartialPacket,
    ) {
        match partial_packet.try_into() {
            Ok(packet) => event_system
                .receiver
                .push(ReceiverEvent::ConnectionAccepted { packet }),
            Err(fail) => event_system.api.push(AntarcEvent::Fail(fail)),
        }
    }

    pub fn received_data_transfer(event_system: &mut EventSystem, partial_packet: PartialPacket) {
        match partial_packet.try_into() {
            Ok(packet) => event_system
                .receiver
                .push(ReceiverEvent::DataTransfer { packet }),
            Err(fail) => event_system.api.push(AntarcEvent::Fail(fail)),
        }
    }

    pub fn received_heartbeat(event_system: &mut EventSystem, partial_packet: PartialPacket) {
        match partial_packet.try_into() {
            Ok(packet) => event_system
                .receiver
                .push(ReceiverEvent::Heartbeat { packet }),
            Err(fail) => event_system.api.push(AntarcEvent::Fail(fail)),
        }
    }

    pub fn received_fragment(event_system: &mut EventSystem, partial_packet: PartialPacket) {
        todo!()
    }

    pub fn poll(&mut self) -> Vec<AntarcEvent> {
        for partial_packet in self.receiver_pipe.drain(..) {
            match partial_packet.header_info.status_code {
                1..=25 => Self::received_connection_request(&mut self.event_pipe, partial_packet),
                26..=50 => Self::received_connection_accepted(&mut self.event_pipe, partial_packet),
                51..=75 => Self::received_data_transfer(&mut self.event_pipe, partial_packet),
                76..=100 => Self::received_heartbeat(&mut self.event_pipe, partial_packet),
                101..=125 => Self::received_fragment(&mut self.event_pipe, partial_packet),
                other => panic!("{} code is not being used!", other),
            }
        }

        for received in self.event_pipe.receiver.drain(..) {
            match received {
                ReceiverEvent::ConnectionRequest { packet } => {
                    debug!("server: received connection request {:#?}.", packet);
                    // NOTE(alex): A host only changes RequestingConnection -> AwaitingConnectionAck
                    // after a connection accepted/denied packet is sent.
                    if self
                        .connection_system
                        .already_requesting_connection(&packet.state.source)
                        || self
                            .connection_system
                            .already_awaiting_connection_ack(&packet.state.source)
                        || self
                            .connection_system
                            .already_connected(&packet.state.source)
                    {
                        warn!(
                            "server: host already in another state, skipping {:#?}.",
                            packet
                        );
                        continue;
                    }

                    let new_host = Host::new(packet.state.source);
                    self.connection_system
                        .requesting_connection
                        .insert(new_host.address, new_host);
                }
                ReceiverEvent::ConnectionAccepted { packet } => {
                    warn!(
                        "server: received connection accepted, skipping {:#?}.",
                        packet
                    );
                    continue;
                }
                ReceiverEvent::DataTransfer { packet } => {
                    debug!("server: received data transfer {:#?}.", packet);

                    let connection_id = packet.message.connection_id;
                    let payload = packet.message.payload;

                    if let Some(host) = self.connection_system.connected.get_mut(&connection_id) {
                        debug!("server: host is connected {:#?}.", host);

                        host.remote_ack_tracker = packet.header_info.sequence.get();
                        host.local_ack_tracker = packet.header_info.ack;
                    } else if let Some(mut host) = self
                        .connection_system
                        .awaiting_connection_ack
                        .remove(&connection_id)
                    {
                        debug!("server: host is awaiting connection ack {:#?}.", host);

                        host.remote_ack_tracker = packet.header_info.sequence.get();
                        host.local_ack_tracker = packet.header_info.ack;
                        let connected = host.connected(connection_id);

                        self.connection_system
                            .connected
                            .insert(connection_id, connected);

                        self.event_pipe.api.push(AntarcEvent::DataTransfer {
                            connection_id,
                            payload,
                        });
                    }
                }
                ReceiverEvent::Heartbeat { packet } => {
                    debug!("server: received heartbeat {:#?}.", packet);

                    let connection_id = packet.message.connection_id;

                    if let Some(host) = self.connection_system.connected.get_mut(&connection_id) {
                        debug!("server: host is connected {:#?}", host);

                        host.remote_ack_tracker = packet.header_info.sequence.get();
                        host.local_ack_tracker = packet.header_info.ack;
                    } else if let Some(mut host) = self
                        .connection_system
                        .awaiting_connection_ack
                        .remove(&connection_id)
                    {
                        debug!("server: host is awaiting connection ack {:#?}", host);

                        host.remote_ack_tracker = packet.header_info.sequence.get();
                        host.local_ack_tracker = packet.header_info.ack;
                        let connected = host.connected(connection_id);

                        self.connection_system
                            .connected
                            .insert(connection_id, connected);
                    }
                }
            }
        }

        self.event_pipe.api.drain(..).collect()
    }

    pub fn on_received(&mut self, raw_packet: RawPacket) -> Result<(), ProtocolError> {
        unimplemented!()
    }
}
