use std::{convert::TryInto, time::Duration};

use log::{error, warn};

use crate::{
    events::ProtocolError,
    packets::{raw::RawPacket, received::Received, Handshake, Packet, Transfer},
    Protocol,
};

#[derive(Debug)]
pub struct Client {
    pub last_sent_time: Duration,
}

impl Protocol<Client> {
    pub fn new_client() -> Self {
        todo!()
    }

    // TODO(alex) [high] 2021-06-27: It's becoming painfully clear to me that trying to "force" this
    // idea of "proven at compile time" into rust is not a thing that can be done (with success and
    // future maintenance in mind).
    //
    // The best tool for this job is the `enum` and possibly returning errors from invalid match
    // clauses. The code duplication that happens otherwise is enormous and requires macros to ease
    // the process, but these are not refactor-friendly when the types are so strict.
    //
    // Some of these types are valid and valuable, such as the `Partial` version of a packet, when
    // they're presented as a form of "builder pattern", but trying to overly restrict the other
    // concepts into separate types leads to a bunch more code for even the small things.
    //
    // I think the idea of having separate lists of types for a `Connection`, for example, provides
    // enough value, that going further by trying to force different types on `Packet`s brings
    // frustration rather than benefits.
    //
    // An "ECS" approach, with its "dynamicism", may be able to go around these problems.
    pub fn on_received(&mut self, raw_packet: RawPacket) -> Result<(), ProtocolError> {
        let partial_packet = raw_packet.decode(self.connection_system.packet_id_tracker)?;
        let source = partial_packet.address;
        let connection_id = partial_packet.connection_id;

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
                    Transfer::ConnectionTerminate(packet) => {
                        error!(
                            "Host {:#?} expects `DataTransfer` or `Heartbeat`, but got {:#?}",
                            host, packet
                        );
                        todo!()
                    }
                }
            } else if let Some(host) = self.connection_system.connected.get_mut(&connection_id) {
                match data_packet.carrier {
                    Transfer::DataTransfer(_) => todo!(),
                    Transfer::Heartbeat(_) => todo!(),
                    Transfer::ConnectionTerminate(_) => todo!(),
                }
            }
        } else {
            let handshake_packet: Packet<Received, Handshake> = partial_packet.try_into()?;
            if let Some(host) = self
                .connection_system
                .requesting_connection
                .iter()
                .find(|host| host.address == source)
            {
            } else {
            }
        }

        todo!()
    }
}
