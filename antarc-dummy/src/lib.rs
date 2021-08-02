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
use log::{debug, info};

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
        debug!("dummy poll");

        for scheduled in self.antarc.events.scheduler.drain(..) {
            // TODO(alex) [high] 2021-08-01: Handle the conversion of `Scheduled` into `Packet` by
            // calling some `Protocol::encode` (should be similar to the `decode`).
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
        debug!("dummy poll");

        for scheduled in self.antarc.events.scheduler.drain(..) {
            match scheduled {
                ScheduleEvent::ConnectionRequest { scheduled } => {
                    debug!("preparing to send a connection request {:#?}.", scheduled);

                    // TODO(alex) [high] 2021-08-02: This should be part of the protocol, but how do
                    // I make something generic over this? I should probably start with single
                    // functions, one for each type of packet / peer connection combination.
                    //
                    // TODO(alex) [mid] 2021-08-02: Thinking about how to do reliability for the
                    // `Packet`, instead of adding a new generic to it, I could insert reliable
                    // packets into a `Vec<Packet<Sent, T>>` sort of thing. Maybe wrap it into an
                    // event style enum, like `Scheduled`.
                    let (sequence, ack) = self
                        .antarc
                        .service
                        .requesting_connection
                        .values()
                        .last()
                        .map(|peer| (peer.sequence_tracker, peer.remote_ack_tracker))
                        .unwrap();

                    let packet = scheduled.into_packet(sequence, ack, self.antarc.timer.elapsed());
                    debug!("ready to send {:#?}", packet);

                    {
                        let raw_packet = packet.as_raw();
                        info!(
                            "Sent: {:#?} bytes to {:#?}",
                            raw_packet.bytes.len(),
                            raw_packet.address
                        );
                    }

                    let sent = packet.sent(self.antarc.timer.elapsed());
                    debug!("sent packet {:#?}", sent);
                    // TODO(alex) [vhigh] 2021-08-02: Add this packet to the list of reliable sent.
                }
                ScheduleEvent::ConnectionAccepted { scheduled } => self.antarc.events.api.push(
                    ProtocolError::ScheduleInvalidPeer(scheduled.message.connection_id).into(),
                ),
                ScheduleEvent::ReliableDataTransfer { scheduled } => todo!(),
                ScheduleEvent::ReliableFragment { scheduled } => todo!(),
                ScheduleEvent::UnreliableDataTransfer { scheduled } => todo!(),
                ScheduleEvent::UnreliableFragment { scheduled } => todo!(),
            }
            // TODO(alex) [high] 2021-08-01: Handle the conversion of `Scheduled` into `Packet` by
            // calling some `Protocol::encode` (should be similar to the `decode`).
        }

        self.antarc.poll()
    }
}
