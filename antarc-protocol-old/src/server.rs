#![feature(more_qualified_paths)]

use std::{convert::TryInto, time::Duration};

use log::{debug, warn};

use crate::{
    controls::data_transfer::DataTransfer,
    events::{AntarcEvent, ProtocolError, ReceiverEvent, SenderEvent},
    hosts::Host,
    packets::{
        partial::PartialPacket, raw::RawPacket, received::Received, scheduled::Scheduled,
        ConnectionId, Packet,
    },
    payload::{Fragment, Payload},
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

    /// NOTE(alex): API function that feeds the internal* event pipe, it's called from the outside.
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

    // TODO(alex) [mid] 2021-07-28: Handle packet fragmentation.
    pub fn received_fragment(event_system: &mut EventSystem, partial_packet: PartialPacket) {
        todo!()
    }

    pub fn schedule(&mut self, payload: Payload, destinations: &[ConnectionId]) {
        let fragment = payload.clone().fragment();

        let mut scheduled_list = destinations
            .iter()
            .flat_map(|destination| {
                // TODO(alex) [high] 2021-07-29: We hit yet another barrier due to generic state
                // handling here. There are 2 types of possible packets, fragmented or not, but
                // there is no "dependent type" way of handling this. `packets` has to be a specific
                // type of packet, and I'm just creating it as `<Scheduled, DataTransfer>`, with no
                // indication of fragmentation.
                //
                // The ordeal could be passed forward, but it would be just moving a problem to some
                // other place, there is no stateful way of handling this.
                let mut packets = match fragment.clone() {
                    Fragment::Single(payload) => {
                        let packet = Packet::<Scheduled, DataTransfer>::new(
                            self.connection_system.packet_id_tracker,
                            payload,
                            self.timer.elapsed(),
                            destination.clone(),
                        );

                        vec![packet]
                    }
                    Fragment::Multiple(payloads) => {
                        let packets = payloads
                            .into_iter()
                            .map(|payload| {
                                let packet = Packet::<Scheduled, DataTransfer>::new(
                                    self.connection_system.packet_id_tracker,
                                    payload,
                                    self.timer.elapsed(),
                                    destination.clone(),
                                );

                                packet
                            })
                            .collect::<Vec<_>>();

                        packets
                    }
                };

                let events = packets
                    .drain(..)
                    .map(|packet| SenderEvent::ScheduledDataTransfer { packet })
                    .collect::<Vec<_>>();
                events
            })
            .collect::<Vec<_>>();

        self.event_pipe.sender.append(&mut scheduled_list);
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

        // TODO(alex) [high] 2021-07-28: Create a function that feeds the event pipe for sending
        // packets, then handle these send events.
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
