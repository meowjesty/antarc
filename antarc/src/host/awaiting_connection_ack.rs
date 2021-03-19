use super::{
    connected::Connected, disconnected::Disconnected,
    sending_connection_request::SendingConnectionRequest, Host,
};
use crate::packet::{received::Received, Packet};

#[derive(Debug)]
pub(crate) struct AwaitingConnectionAck {
    pub(crate) attempts: u32,
}

#[derive(Debug)]
pub(crate) struct FailedAwaitingConnectionAck {
    attempts: u32,
    error: String,
}

impl Host<AwaitingConnectionAck> {
    pub(crate) fn poll(mut self) -> Result<Self, Host<FailedAwaitingConnectionAck>> {
        // TODO(alex) 2021-03-19: `if sent.time_sent > RESEND_TIMEOUT_THRESHOLD`
        todo!()
    }

    pub(crate) fn on_received_connection_ack(
        mut self,
        packet: Packet<Received>,
    ) -> Result<Host<Connected>, Host<Disconnected>> {
        // TODO(alex) 2021-03-19: `if accepted else denied`
        todo!()
    }
}

impl Host<FailedAwaitingConnectionAck> {
    pub(crate) fn retry(self) -> Result<Host<SendingConnectionRequest>, Host<Disconnected>> {
        todo!()
    }
}
