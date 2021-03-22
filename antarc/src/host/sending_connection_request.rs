use std::net::UdpSocket;

use super::{awaiting_connection_ack::AwaitingConnectionAck, disconnected::Disconnected, Host};
use crate::packet::{header::Header, to_send::ToSend, Packet, Payload, Sequence};

#[derive(Debug)]
pub(crate) struct SendingConnectionRequest {
    pub(crate) attempts: u32,
}

#[derive(Debug, Default)]
pub(crate) struct FailedSendingConnectionRequest {
    attempts: u32,
    error: String,
}

impl Host<SendingConnectionRequest> {
    pub(crate) fn send(
        mut self,
        socket: &UdpSocket,
    ) -> Result<Host<AwaitingConnectionAck>, Host<FailedSendingConnectionRequest>> {
        let header = Header::connection_request();
        let packet = Packet::<ToSend>::new(header, Payload(vec![0; 10]), self.timer.elapsed());

        let raw_packet = packet.encode().unwrap();

        match socket.send_to(&raw_packet.buffer, self.address) {
            Ok(num_sent) => {
                assert!(num_sent > 0);
                self.sequence_tracker =
                    unsafe { Sequence::new_unchecked(self.sequence_tracker.get() + 1) };

                let sent = packet.sent(self.timer.elapsed());
                self.sent_list.push(sent);

                let awaiting_connection_ack =
                    self.into_new_state(AwaitingConnectionAck { attempts: 0 });
                Ok(awaiting_connection_ack)
            }
            Err(fail) => {
                let failed_to_send_connection_request = FailedSendingConnectionRequest {
                    attempts: self.connection.attempts + 1,
                    error: fail.to_string(),
                };
                let failed_to_send_connection_request =
                    self.into_new_state(failed_to_send_connection_request);
                Err(failed_to_send_connection_request)
            }
        }
    }
}

impl Host<FailedSendingConnectionRequest> {
    pub(crate) fn retry(self) -> Result<Host<SendingConnectionRequest>, Host<Disconnected>> {
        if self.connection.attempts > 10 {
            let disconnected = self.into_new_state(Disconnected);
            Err(disconnected)
        } else {
            let attempts = self.connection.attempts;
            let sending_connection_request =
                self.into_new_state(SendingConnectionRequest { attempts });
            Ok(sending_connection_request)
        }
    }
}
