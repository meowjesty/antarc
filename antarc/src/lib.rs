use core::time::Duration;
use std::net::SocketAddr;

use antarc_protocol::{errors::*, packets::*, peers::*, *};
use log::*;

pub mod client;
pub mod server;

pub use antarc_protocol::{client::*, errors::*, events::*, packets::*, peers::*, server::*, *};

#[derive(Debug)]
pub struct AntarcNet<S: Service> {
    antarc: Protocol<S>,
    address: SocketAddr,
    pub dummy_sender: Vec<Vec<u8>>,
    pub dummy_receiver: Vec<Vec<u8>>,
}

impl<S: Service> AntarcNet<S> {
    pub fn schedule(
        &mut self,
        reliability: ReliabilityType,
        send_to: SendTo,
        payload: Payload,
    ) -> Result<PacketId, ProtocolError> {
        info!(
            "Antarc: scheduling transfer of {:#?} bytes, is reliable {:#?}, to {:#?}.",
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
            "Antarc: scheduling heartbeat, is reliable {:#?}, to {:#?}.",
            reliability, send_to,
        );

        // self.antarc.heartbeat(reliability, send_to)
        todo!()
    }
}
