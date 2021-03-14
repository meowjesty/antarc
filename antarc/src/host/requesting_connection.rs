use std::time::Duration;

use super::{connected::Connected, Host};
use crate::{
    host::ConnectionId,
    packet::{
        ConnectionRequestInfo, Header, HeaderInfo, Packet, Payload, Sequence, ToSend,
        CONNECTION_REQUEST,
    },
};

#[derive(Debug, Default)]
pub(crate) struct ConnectionRequestTimedOut {
    error: String,
    retries: u32,
}

#[derive(Debug, Default)]
pub(crate) struct RequestingConnection {
    retries: u32,
}

#[derive(Debug, Default)]
pub(crate) struct ConnectionDenied {
    error: String,
}

impl Host<RequestingConnection> {
    /// TODO(alex) 2021-03-13: I think this function could be just some code in the `Client`, to
    /// avoid further complications here.
    pub(crate) fn poll(mut self) -> Result<Self, Host<ConnectionRequestTimedOut>> {
        assert!(self.sent_list.last().is_some());
        let last_sent = self.sent_list.last().unwrap();
        if last_sent.state.time_sent < self.timer.elapsed() - Duration::from_millis(1000) {
            let failed_to_receive_connection_ack_in_time = ConnectionRequestTimedOut {
                retries: self.connection.retries,
                error: format!("Failed to receive connection ack in time {:#?}.", self),
            };

            Err(self.into_new_state(failed_to_receive_connection_ack_in_time))
        } else {
            Ok(self)
        }
    }

    pub(crate) fn on_received_connection_ack(
        mut self,
        buffer: &[u8],
    ) -> Result<Host<Connected>, Host<ConnectionDenied>> {
        let packet = Packet::decode(buffer, self.timer.elapsed()).unwrap();

        if let Header::ConnectionAccepted(connection_accepted_info) = &packet.header.clone() {
            if self.on_receive_ack(&packet) {
                self.internals.push(packet.internald(self.timer.elapsed()));

                let connected = self.into_new_state(Connected {
                    connection_id: connection_accepted_info.connection_id,
                });
                Ok(connected)
            } else {
                panic!("{:?}", format!("Failed to connect to {:#?}.", self));
            }
        } else if let Header::ConnectionDenied(connection_denied_info) = &packet.header.clone() {
            let mut denied = self.into_new_state(ConnectionDenied {
                error: format!("{:#?} with connection denied for", packet,),
            });
            denied
                .internals
                .push(packet.internald(denied.timer.elapsed()));
            Err(denied)
        } else {
            panic!(
                "{:?}",
                format!(
                    "Expected connection accepted or denied packet, got {:#?} for {:#?}",
                    packet, self
                )
            )
        }
    }
}
