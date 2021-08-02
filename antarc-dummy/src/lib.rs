use std::net::SocketAddr;

use antarc_protocol::events::{ProtocolError, ScheduleEvent};
// TODO(alex) [mid] 2021-07-31: Create a dummy network manager implementation, that just uses
// buffers to move data, no sockets involved!
pub use antarc_protocol::{
    client::Client,
    events::AntarcEvent,
    packets::{ConnectionId, Payload},
    peers::SendTo,
    server::Server,
    Protocol,
};
use log::{debug, info, warn};

#[derive(Debug)]
pub struct DummyManager<Service> {
    antarc: Protocol<Service>,
    address: SocketAddr,
}

impl DummyManager<Server> {
    pub fn new_server(address: SocketAddr) -> Self {
        debug!("dummy new server");
        let antarc = Protocol::new_server();
        Self { antarc, address }
    }

    pub fn schedule(&mut self, reliable: bool, send_to: SendTo, payload: Payload) {
        debug!("dummy schedule");
        self.antarc.schedule(reliable, send_to, payload);
    }

    pub fn accept_connection(&mut self, connection_id: ConnectionId) {
        debug!("dummy accept connection");
        self.antarc.accept_connection(connection_id);
    }

    pub fn poll(&mut self) -> std::vec::Drain<AntarcEvent> {
        debug!("dummy server poll");

        for scheduled in self.antarc.scheduler().drain(..) {
            match scheduled {
                ScheduleEvent::ConnectionRequest { scheduled } => {
                    warn!("Invalid packet type {:#?} scheduled for client.", scheduled);
                    self.antarc
                        .events
                        .api
                        .push(ProtocolError::ScheduledInvalidPeer(scheduled.packet_id).into());
                }
                ScheduleEvent::ConnectionAccepted { scheduled } => {
                    debug!("preparing to send a connection request {:#?}.", scheduled);

                    let packet = self.antarc.prepare_connection_accepted(scheduled);
                    debug!("ready to send {:#?}", packet);

                    // NOTE(alex): Dummy send.
                    {
                        let raw_packet = packet.as_raw();
                        info!(
                            "Sent: {:#?} bytes to {:#?}",
                            raw_packet.bytes.len(),
                            raw_packet.address
                        );
                    }

                    self.antarc.sent_connection_accepted(packet);
                }
                ScheduleEvent::ReliableDataTransfer { scheduled } => todo!(),
                ScheduleEvent::ReliableFragment { scheduled } => todo!(),
                ScheduleEvent::UnreliableDataTransfer { scheduled } => todo!(),
                ScheduleEvent::UnreliableFragment { scheduled } => todo!(),
            }
        }

        self.antarc.poll()
    }
}

impl DummyManager<Client> {
    pub fn new_client(address: SocketAddr) -> Self {
        let antarc = Protocol::new_client();
        Self { antarc, address }
    }

    // TODO(alex) [mid] 2021-08-02: Remember to return a `PacketId` from scheduling functions, so
    // that the user may cancel a packet.
    pub fn schedule(&mut self, reliable: bool, payload: Payload) {
        info!(
            "Scheduling transfer of {:#?} bytes, is reliable {:#?}.",
            payload.len(),
            reliable
        );
        self.antarc.schedule(reliable, payload);
    }

    pub fn connect(&mut self, remote_address: SocketAddr) -> Result<(), ProtocolError> {
        info!("Connect to {:#?}.", remote_address);
        self.antarc.connect(remote_address)
    }

    pub fn poll(&mut self) -> std::vec::Drain<AntarcEvent> {
        debug!("dummy client poll");

        for scheduled in self.antarc.scheduler().drain(..) {
            match scheduled {
                ScheduleEvent::ConnectionRequest { scheduled } => {
                    debug!("preparing to send a connection request {:#?}.", scheduled);

                    let packet = self.antarc.prepare_connection_request(scheduled);
                    debug!("ready to send {:#?}", packet);

                    // NOTE(alex): Dummy send.
                    {
                        let raw_packet = packet.as_raw();
                        info!(
                            "Sent: {:#?} bytes to {:#?}",
                            raw_packet.bytes.len(),
                            raw_packet.address
                        );
                    }

                    self.antarc.sent_connection_request(packet);
                }
                ScheduleEvent::ConnectionAccepted { scheduled } => {
                    warn!("Invalid packet type {:#?} scheduled for client.", scheduled);
                    self.antarc
                        .events
                        .api
                        .push(ProtocolError::ScheduledInvalidPeer(scheduled.packet_id).into());
                }
                ScheduleEvent::ReliableDataTransfer { scheduled } => todo!(),
                ScheduleEvent::ReliableFragment { scheduled } => todo!(),
                ScheduleEvent::UnreliableDataTransfer { scheduled } => todo!(),
                ScheduleEvent::UnreliableFragment { scheduled } => todo!(),
            }
        }

        self.antarc.poll()
    }
}
