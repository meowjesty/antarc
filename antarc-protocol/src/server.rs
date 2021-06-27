use std::{convert::TryInto, time::Duration};

use crate::{
    events::ProtocolError,
    packets::{raw::RawPacket, received::Received, ConnectionId, Handshake, Packet, Transfer},
    Protocol,
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

    pub fn on_received(&mut self, raw_packet: RawPacket) -> Result<(), ProtocolError> {
        let partial_packet = raw_packet.decode(self.connection_system.packet_id_tracker)?;
        let source = partial_packet.address;
        let connection_id = partial_packet.connection_id;

        // TODO(alex) [vhigh] 2021-06-26: Must match on whatever type is contained in
        // the partial packet.

        if let Some(connection_id) = connection_id {
            let data_packet: Packet<Received, Transfer> = partial_packet.try_into()?;

            if let Some(host) = self
                .connection_system
                .awaiting_connection_ack
                .remove(&connection_id)
            {
                match data_packet.carrier {
                    Transfer::DataTransfer(_) => todo!(),
                    Transfer::Heartbeat(_) => todo!(),
                }
            } else if let Some(host) = self.connection_system.connected.get_mut(&&connection_id) {
                match data_packet.carrier {
                    Transfer::DataTransfer(_) => todo!(),
                    Transfer::Heartbeat(_) => todo!(),
                }
            }
        } else {
            if let Some(host) = self
                .connection_system
                .requesting_connection
                .iter()
                .find(|host| host.address == source)
            {
                let handshake_packet: Packet<Received, Handshake> = partial_packet.try_into()?;
            } else {
            }
        }

        todo!()
    }
}
