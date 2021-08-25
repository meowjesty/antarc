use core::time::Duration;
use std::net::{SocketAddr, UdpSocket};

use log::*;

pub mod client;
pub mod server;

pub use antarc_protocol::{
    client::*,
    errors::*,
    events::*,
    packets::{scheduled::*, *},
    peers::*,
    server::*,
    service_traits::*,
    *,
};

#[derive(Debug)]
pub struct AntarcNet<S: Service> {
    antarc: Protocol<S>,
    address: SocketAddr,
    socket: UdpSocket,
    recv_buffer: Vec<u8>,
}

impl<S: Service> AntarcNet<S> {
    pub fn schedule(
        &mut self,
        reliability: ReliabilityType,
        send_to: SendTo,
        payload: Payload,
    ) -> Result<PacketId, ProtocolError> {
        info!(
            "{}: scheduling transfer of {:#?} bytes, is reliable {:#?}, to {:#?}.",
            S::DEBUG_NAME,
            payload.len(),
            reliability,
            send_to,
        );
        self.antarc.schedule(reliability, send_to, payload)
    }

    pub fn heartbeat(
        &mut self,
        reliability: ReliabilityType,
        send_to: SendTo,
    ) -> Result<PacketId, ProtocolError> {
        info!(
            "{}: scheduling heartbeat, is reliable {:#?}, to {:#?}.",
            S::DEBUG_NAME,
            reliability,
            send_to,
        );

        self.antarc.heartbeat(reliability, send_to)
    }

    fn poll_reliable_fragment(&mut self) {
        for scheduled in self.antarc.drain_reliable_fragment(..).collect::<Vec<_>>() {
            debug!("{}: preparing to send {:#?}.", S::DEBUG_NAME, scheduled);
            // TODO(alex) [low] 2021-08-17: The whole chain for this function is filled with
            // unneccesary duplication. Creation of unreliable / reliable packets are equal, the
            // only differences in reliability come AFTER the packet is sent.
            //
            // This means that these `create_` functions could take `<R: Reliability>` or something
            // generic like that, to avoid the need for 2 distinct function definitions.
            //
            // The duplication also applies to other `create_x` functions that are basically the
            // same for both Client and Client, but right now are completely separated.
            //
            // Most `drain_x` functions could be done at `impl<S: Service> Protocol<S>`.
            //
            // TODO(alex) [low] 2021-08-17: Could we get rid of duplication by passing down a
            // function callback?
            // fn common_create_data_transfer(scheduled, fn_create_reliable_data_transfer);
            let packet = self.antarc.create_reliable_fragment(scheduled);
            debug!("{}: ready to send {:#?}.", S::DEBUG_NAME, packet);

            // NOTE(alex): Udp send.
            {
                let raw_packet = packet.as_raw();
                self.socket
                    .send_to(&raw_packet.bytes, raw_packet.address)
                    .unwrap();
            }

            self.antarc.sent_fragment(packet, ReliabilityType::Reliable);
        }
    }

    fn poll_reliable_data_transfer(&mut self) {
        for scheduled in self
            .antarc
            .drain_reliable_data_transfer(..)
            .collect::<Vec<_>>()
        {
            debug!("{}: preparing to send {:#?}.", S::DEBUG_NAME, scheduled);
            // TODO(alex) [low] 2021-08-17: The whole chain for this function is filled with
            // unneccesary duplication. Creation of unreliable / reliable packets are equal, the
            // only differences in reliability come AFTER the packet is sent.
            //
            // This means that these `create_` functions could take `<R: Reliability>` or something
            // generic like that, to avoid the need for 2 distinct function definitions.
            //
            // The duplication also applies to other `create_x` functions that are basically the
            // same for both Client and Client, but right now are completely separated.
            //
            // Most `drain_x` functions could be done at `impl<S: Service> Protocol<S>`.
            //
            // TODO(alex) [low] 2021-08-17: Could we get rid of duplication by passing down a
            // function callback?
            // fn common_create_data_transfer(scheduled, fn_create_reliable_data_transfer);
            let packet = self.antarc.create_reliable_data_transfer(scheduled);
            debug!("{}: ready to send {:#?}.", S::DEBUG_NAME, packet);

            // NOTE(alex): Udp send.
            {
                let raw_packet = packet.as_raw();
                self.socket
                    .send_to(&raw_packet.bytes, raw_packet.address)
                    .unwrap();
            }

            self.antarc
                .sent_data_transfer(packet, ReliabilityType::Reliable);
        }
    }

    fn poll_unreliable_fragment(&mut self) {
        for scheduled in self
            .antarc
            .drain_unreliable_fragment(..)
            .collect::<Vec<_>>()
        {
            debug!("{}: preparing to send {:#?}.", S::DEBUG_NAME, scheduled);
            let packet = self.antarc.create_unreliable_fragment(scheduled);
            debug!("{}: ready to send {:#?}.", S::DEBUG_NAME, packet);

            // NOTE(alex): Udp send.
            {
                let raw_packet = packet.as_raw();
                self.socket
                    .send_to(&raw_packet.bytes, raw_packet.address)
                    .unwrap();
            }

            self.antarc
                .sent_fragment(packet, ReliabilityType::Unreliable);
        }
    }

    fn poll_unreliable_data_transfer(&mut self) {
        for scheduled in self
            .antarc
            .drain_unreliable_data_transfer(..)
            .collect::<Vec<_>>()
        {
            debug!("{}: preparing to send {:#?}.", S::DEBUG_NAME, scheduled);
            let packet = self.antarc.create_unreliable_data_transfer(scheduled);
            debug!("{}: ready to send {:#?}.", S::DEBUG_NAME, packet);

            // NOTE(alex): Udp send.
            {
                let raw_packet = packet.as_raw();
                self.socket
                    .send_to(&raw_packet.bytes, raw_packet.address)
                    .unwrap();
            }

            self.antarc
                .sent_data_transfer(packet, ReliabilityType::Unreliable);
        }
    }

    fn poll_retry_data_transfer(&mut self) {
        if let Some(reliable_packet) = self.antarc.retry_reliable_data_transfer() {
            debug!("{}: ready to re-send {:#?}", S::DEBUG_NAME, reliable_packet);

            // NOTE(alex): Udp send.
            {
                let raw_packet = reliable_packet.as_raw();
                self.socket
                    .send_to(&raw_packet.bytes, raw_packet.address)
                    .unwrap();
            }

            self.antarc
                .sent_data_transfer(reliable_packet, ReliabilityType::Reliable);
        }
    }

    fn poll_retry_fragment(&mut self) {
        if let Some(reliable_packet) = self.antarc.retry_reliable_fragment() {
            debug!("{}: ready to re-send {:#?}", S::DEBUG_NAME, reliable_packet);

            // NOTE(alex): Udp send.
            {
                let raw_packet = reliable_packet.as_raw();
                self.socket
                    .send_to(&raw_packet.bytes, raw_packet.address)
                    .unwrap();
            }

            self.antarc
                .sent_fragment(reliable_packet, ReliabilityType::Reliable);
        }
    }

    fn poll_unreliable_heartbeat(&mut self) {
        for scheduled in self
            .antarc
            .drain_unreliable_heartbeat(..)
            .collect::<Vec<_>>()
        {
            debug!("{}: preparing to send {:#?}.", S::DEBUG_NAME, scheduled);
            let packet = self.antarc.create_unreliable_heartbeat(scheduled);
            debug!("{}: ready to send {:#?}.", S::DEBUG_NAME, packet);

            // NOTE(alex): Udp send.
            {
                let raw_packet = packet.as_raw();
                self.socket
                    .send_to(&raw_packet.bytes, raw_packet.address)
                    .unwrap();
            }

            self.antarc
                .sent_heartbeat(packet, ReliabilityType::Unreliable);
        }
    }

    fn poll_reliable_heartbeat(&mut self) {
        for scheduled in self.antarc.drain_reliable_heartbeat(..).collect::<Vec<_>>() {
            debug!("{}: preparing to send {:#?}.", S::DEBUG_NAME, scheduled);
            let packet = self.antarc.create_reliable_heartbeat(scheduled);
            debug!("{}: ready to send {:#?}.", S::DEBUG_NAME, packet);

            // NOTE(alex): Udp send.
            {
                let raw_packet = packet.as_raw();
                self.socket
                    .send_to(&raw_packet.bytes, raw_packet.address)
                    .unwrap();
            }

            self.antarc
                .sent_heartbeat(packet, ReliabilityType::Reliable);
        }
    }
}
