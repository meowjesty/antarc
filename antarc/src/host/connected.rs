use std::{net::UdpSocket, time::Duration};

use super::Host;
use crate::{
    host::ConnectionId,
    packet::{
        ConnectionRequestInfo, DataTransferInfo, Header, HeaderInfo, Packet, Payload, Sequence,
        ToSend, CONNECTION_REQUEST, DATA_TRANSFER,
    },
    AntarcResult,
};

#[derive(Debug)]
pub(crate) struct Connected {
    pub(crate) connection_id: ConnectionId,
}

impl Host<Connected> {
    /// NOTE(alex) 2021-03-03: `receive` cannot be done in the `Host`, as the remote address is
    /// unknown until `socket.recv_from` is called, rendering it impossible to do here. From the
    /// `Client` perspective, it would still be doable, as it only has to check if it belongs to
    /// the unique server `Host`, but from the `Server`'s side, it's neccessary to find the correct
    /// `Host::address` for the received address.
    ///
    /// TODO(alex) 2021-03-03: I'm thinking that every `Host::receive` will be the same, or at least
    /// they are for now, so migrating it into the more generic `impl<State> Host<State>` might be
    /// fine.
    pub(crate) fn on_receive(&mut self, buffer: &[u8]) -> AntarcResult<()> {
        let packet = Packet::decode(buffer, self.timer.elapsed())?;
        self.on_receive_ack(&packet);

        self.received_list.push(packet);
        Ok(())
    }

    /// TODO(alex) 2021-03-03: In contrast, `Host::send` looks perfectly doable, as the protocol
    /// only wants to send packets to hosts it knows of.
    pub(crate) fn send(&mut self, socket: &UdpSocket) -> AntarcResult<usize> {
        if let Some(payload) = self
            .priority_queue
            .pop_front()
            .or(self.send_queue.pop_front())
        {
            let Connected { connection_id } = self.connection;
            let header_info = HeaderInfo {
                sequence: self.sequence_tracker,
                ack: self.ack_tracker,
                past_acks: self.past_acks_tracker,
            };
            let data_transfer_info = DataTransferInfo {
                status_code: DATA_TRANSFER,
                connection_id,
                header_info,
            };
            let header = Header::DataTransfer(data_transfer_info);

            // TODO(alex) 2021-03-03: This part will be the same for every `Host<State>`, it makes
            // sense to move in into the `impl<State> Host<State>` as a private part.
            {
                let to_send = Packet::<ToSend>::new(header, payload, self.timer.elapsed());
                let to_send_raw = to_send.encode()?;

                let num_sent = socket
                    .send_to(&to_send_raw.buffer, self.address)
                    .map_err(|fail| fail.to_string())?;

                self.sent_list.push(to_send.sent(self.timer.elapsed()));

                // NOTE(alex) 2021-03-02: Sequence is only incremented if a packet was successfully
                // sent, otherwise the protocol could end up in an incosistent state with a remote
                // `Host` thinking that some packets were lost, even though these packets were never
                // actually sent. This is why a `Packet<ToSend>` is created in this function and not
                // stored anywhere else, to prevent these inconsistencies.
                self.sequence_tracker = Sequence::new(self.sequence_tracker.get() + 1).unwrap();

                return Ok(num_sent);
            }
        }

        Ok(0)
    }
}
