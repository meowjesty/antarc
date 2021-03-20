use super::{
    connected::Connected, disconnected::Disconnected,
    sending_connection_request::SendingConnectionRequest, Host, CONNECTION_TIMEOUT_THRESHOLD,
    RESEND_TIMEOUT_THRESHOLD,
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

#[derive(Debug)]
enum ConnectionWentWrong {
    Denied,
    InvalidHeader(Packet<Received>),
}

#[derive(Debug)]
pub(crate) struct FailedConnectionWentWrong {
    attempts: u32,
    reason: ConnectionWentWrong,
}

impl Host<AwaitingConnectionAck> {
    pub(crate) fn poll(self) -> Result<Self, Host<FailedAwaitingConnectionAck>> {
        let connection_request = self.sent_list.last().unwrap();
        if connection_request.state.time_sent < RESEND_TIMEOUT_THRESHOLD {
            Ok(self)
        } else {
            let failed_waiting_connection_ack = FailedAwaitingConnectionAck {
                attempts: self.connection.attempts + 1,
                error: format!("Waited too long for a connection ack."),
            };
            let failed_waiting_connection_ack = self.into_new_state(failed_waiting_connection_ack);
            Err(failed_waiting_connection_ack)
        }
    }

    pub(crate) fn on_received_connection_ack(
        self,
        packet: Packet<Received>,
    ) -> Result<Host<Connected>, Host<FailedConnectionWentWrong>> {
        use crate::packet::header::Header::*;

        match &packet.header {
            ConnectionDenied(connection_denied_info) => {
                let attempts = self.connection.attempts;
                let failed_connection_went_wrong = FailedConnectionWentWrong {
                    attempts,
                    reason: ConnectionWentWrong::Denied,
                };
                let mut failed_connection_went_wrong =
                    self.into_new_state(failed_connection_went_wrong);

                let packet = packet.internald(failed_connection_went_wrong.timer.elapsed());
                failed_connection_went_wrong.internals.push(packet);
                Err(failed_connection_went_wrong)
            }
            ConnectionAccepted(connection_accepted_info) => {
                let mut connected = self.into_new_state(Connected {
                    connection_id: connection_accepted_info.connection_id,
                });

                let packet = packet.internald(connected.timer.elapsed());
                connected.internals.push(packet);
                Ok(connected)
            }
            invalid_header => {
                let attempts = self.connection.attempts;
                let failed_connection_went_wrong = FailedConnectionWentWrong {
                    attempts,
                    reason: ConnectionWentWrong::InvalidHeader(packet),
                };
                let failed_connection_went_wrong =
                    self.into_new_state(failed_connection_went_wrong);
                Err(failed_connection_went_wrong)
            }
        }
    }
}

impl Host<FailedConnectionWentWrong> {
    pub(crate) fn done(self) -> Host<Disconnected> {
        match self.connection.reason {
            ConnectionWentWrong::Denied => {
                let disconnected = self.into_new_state(Disconnected);
                disconnected
            }
            ConnectionWentWrong::InvalidHeader(_) => {
                let disconnected = self.into_new_state(Disconnected);
                disconnected
            }
        }
    }
}

impl Host<FailedAwaitingConnectionAck> {
    pub(crate) fn retry(self) -> Result<Host<SendingConnectionRequest>, Host<Disconnected>> {
        let connection_request = self.sent_list.last().unwrap();
        // TODO(alex) 2021-03-20: We won't ever enter this, as the `time_sent` will be overwritten
        // when the retry request is sent. It's lacking a way of keeping track of the first time the
        // host sends the connection request. Probably something that will keep passing around, like
        // the `attempts`.
        if connection_request.state.time_sent < CONNECTION_TIMEOUT_THRESHOLD {
            let disconnected = self.into_new_state(Disconnected);
            Err(disconnected)
        } else {
            let attempts = self.connection.attempts + 1;
            let sending_connection_request =
                self.into_new_state(SendingConnectionRequest { attempts });
            Ok(sending_connection_request)
        }
    }
}
