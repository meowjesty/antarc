use std::{
    collections::VecDeque,
    marker::PhantomData,
    net::{SocketAddr, UdpSocket},
    num::{NonZeroU16, NonZeroU32},
    time::{Duration, Instant},
};

use crate::{
    exponential_moving_average,
    packet::{
        Acked, ConnectionAcceptedInfo, ConnectionId, ConnectionRequestInfo, DataTransferInfo,
        Footer, Header, HeaderInfo, Internal, Packet, Payload, Received, Retrieved, Sent, Sequence,
        ToSend, CONNECTION_ACCEPTED, CONNECTION_REQUEST, DATA_TRANSFER,
    },
    AntarcResult, PROTOCOL_ID,
};

pub(crate) mod connected;
pub(crate) mod disconnected;
pub(crate) mod requesting_connection;

/// NOTE(alex) 2021-03-11: Macro to map from some `Error`, similar to the `Result::map_err` closure,
/// but avoids capturing a moved value. Without this pattern, trying to call `map_err` will move
/// `self`, and end up as a borrow of a moved value.
///
/// This doesn't work:
///
/// ```
/// let new_state_foo = foo.to_new_state().map_err(|fail| UnmoveError {
///     err: fail,
///     unmoved: self,
/// })?;
/// ```
///
/// But `match`ing works:
///
/// ```
/// let new_state_foo = match foo.to_new_state() {
///     Ok(new_state) => new_state,
///     Err(fail) => {
///         return Err(UnmoveErr {
///             err: fail,
///             unmoved: self,
///         })
///     }
/// };
/// ```
macro_rules! unmove_on_error {
    ($result: expr, $error_type: ident, $this: expr) => {{
        match $result {
            Ok(success) => success,
            Err(fail) => {
                return Err($error_type {
                    err: fail.to_string(),
                    unmoved: $this,
                })
            }
        }
    }};
}

/// TODO(alex) 2021-01-29: Think of `Sessions / Channels` when wondering about connections, it helps
/// when trying to figure out how to keep alive a session (connection), how the communication
/// between hosts occur (channels trasnfer packets), and gives more struct names for similar things.
///
/// TODO(alex) 2021-03-08: Is it possible to delegate more responsibility to the `Host`? We need to
/// use these connection state values better, but how exactly? `AckingConnect` in particular is new,
/// and not used anywhere yet.

#[derive(Debug, Default)]
pub(crate) struct FailedToReceiveConnectionAckInTime {
    retries: u32,
    error: String,
}

#[derive(Debug)]
pub(crate) struct AcceptingConnection {
    connection_id: ConnectionId,
}

#[derive(Debug)]
pub(crate) struct FailedToSendConnectionAccepted {
    connection_id: ConnectionId,
    error: String,
}

#[derive(Debug)]
pub(crate) struct AwaitingConnectionAckFromHost(ConnectionId);

#[derive(Debug, Default)]
pub(crate) struct FailedToConnect {
    error: String,
}

pub(crate) const RESEND_TIMEOUT_THRESHOLD: Duration = Duration::from_millis(500);
pub(crate) const CONNECTION_TIMEOUT_THRESHOLD: Duration = Duration::new(2, 0);

#[derive(Debug)]
pub(crate) struct Host<ConnectionState> {
    pub(crate) address: SocketAddr,
    /// TODO(alex) 2021-02-26: Should each `Host` have its own `Instant` to keep track of timings?
    /// This is not a hard requirement, but many functions will end up using the `Instant` to
    /// calculate time related things, so it might as well be in the struct.
    pub(crate) timer: Instant,

    /// NOTE(alex) 2021-03-02: `sequence` is incremented only after a packet is successfully sent
    /// (`Packet<Sent>`), this is done to prevent remote `Host`s from thinking that some packets
    /// were lost, even in the case of them never being sent.
    pub(crate) sequence_tracker: Sequence,
    pub(crate) ack_tracker: u32,
    pub(crate) past_acks_tracker: u16,
    /// TODO(alex) 2021-02-13: Do not flood the network, find a way to check if the `rtt` is
    /// increasing due to us flooding the network with packets.
    pub(crate) rtt: Duration,
    pub(crate) received_list: Vec<Packet<Received>>,
    pub(crate) retrieved: Vec<Packet<Retrieved>>,
    pub(crate) internals: Vec<Packet<Internal>>,
    pub(crate) send_queue: VecDeque<Payload>,
    /// TODO(alex) 2021-02-28: This behaves pretty much like the `send_queue`, except it takes
    /// priority everytime we check for a `Packet<ToSend>` to send. We're lacking an API to allow
    /// the user to mark a packet with priority to be put in here.
    pub(crate) priority_queue: VecDeque<Payload>,
    pub(crate) sent_list: Vec<Packet<Sent>>,
    pub(crate) acked_list: Vec<Packet<Acked>>,
    pub(crate) connection: ConnectionState,
}

#[derive(Debug)]
pub(crate) struct HostError<ConnectionState> {
    pub(crate) err: String,
    pub(crate) unmoved: Host<ConnectionState>,
}

impl<State> Host<State> {
    #[inline]
    fn into_new_state<NewState>(self, connection: NewState) -> Host<NewState> {
        Host {
            address: self.address,
            timer: self.timer,
            sequence_tracker: self.sequence_tracker,
            ack_tracker: self.ack_tracker,
            past_acks_tracker: self.past_acks_tracker,
            rtt: self.rtt,
            received_list: self.received_list,
            retrieved: self.retrieved,
            internals: self.internals,
            send_queue: self.send_queue,
            priority_queue: self.priority_queue,
            sent_list: self.sent_list,
            acked_list: self.acked_list,
            connection,
        }
    }

    /// TODO(alex) 2021-02-25: Use the `received.past_acks` to ack multiple `Packet<Sent>` that
    /// might still be lingering in the `sent_list`.
    pub(crate) fn on_receive_ack(&mut self, packet: &Packet<Received>) -> bool {
        self.ack_tracker = packet.header.get_sequence().get();

        if let Some((index, _)) = self
            .sent_list
            .iter()
            .enumerate()
            .find(|(_, sent)| packet.header.get_ack() == sent.header.get_sequence().get())
        {
            let sent = self.sent_list.remove(index);
            let acked = sent.acked(self.timer.elapsed());

            let delta_rtt: Duration = acked.state.time_acked - acked.state.time_sent;
            self.rtt = exponential_moving_average(delta_rtt, self.rtt);

            self.acked_list.push(acked);

            true
        } else {
            false
        }
    }
}

// impl Host<FailedToConnect> {
//     pub(crate) fn retry(
//         mut self,
//         socket: &UdpSocket,
//     ) -> Result<Host<RequestingConnection>, Host<Disconnected>> {
//         let requesting_connection = RequestingConnection { retries: 1 };
//         let requesting_connection = self.into_new_state(requesting_connection);
//         requesting_connection.retry(socket)

/*
self.sequence_tracker = unsafe { Sequence::new_unchecked(1) };
if self.connection.retries > 10 {
    return Err(format!(
        "Maximum number of connection retries {:#?} reached for {:#?}.",
        self.connection.retries, self
    ));
}
// NOTE(alex) 2021-03-07: Discard the lost packet, we don't care to keep data about lost
// internals during connection phase.
self.sent_list.pop();

let header_info = HeaderInfo {
    sequence: unsafe { Sequence::new_unchecked(1) },
    ack: 0,
    past_acks: 0,
};
let connection_request_info = ConnectionRequestInfo {
    header_info,
    status_code: CONNECTION_REQUEST,
};
let header = Header::ConnectionRequest(connection_request_info);
let packet = Packet::<ToSend>::new(header, Payload(vec![0; 10]), self.timer.elapsed());

let num_sent = socket
    .send_to(&packet.encode()?.buffer, self.address)
    .map_err(|fail| fail.to_string())?;
assert!(num_sent > 0);
self.sequence_tracker = Sequence::new(self.sequence_tracker.get() + 1).unwrap();

// self.on_sent(packet);
self.connection.retries += 1;

Ok(num_sent)
*/
// }
// }

/*
impl Host<AcceptingConnection> {
    /// TODO(alex) 2021-03-09: `Server`-side only function.
    pub(crate) fn ack_connection(
        mut self,
        socket: &UdpSocket,
    ) -> Result<Host<AwaitingConnectionAckFromHost>, Host<FailedToSendConnectionAccepted>> {
        let header_info = HeaderInfo {
            sequence: self.sequence_tracker,
            ack: self.ack_tracker,
            past_acks: self.past_acks_tracker,
        };
        let connection_accepted_info = ConnectionAcceptedInfo {
            connection_id: self.connection.0,
            header_info,
            status_code: CONNECTION_REQUEST,
        };
        let header = Header::ConnectionAccepted(connection_accepted_info);
        let packet = Packet::<ToSend>::new(header, Payload(vec![0xb; 10]), self.timer.elapsed());

        let num_sent = socket
            .send_to(&packet.encode()?.buffer, self.address)
            .map_err(|fail| fail.to_string())?;
        assert!(num_sent > 0);
        self.sequence_tracker = unsafe { Sequence::new_unchecked(self.sequence_tracker.get() + 1) };

        let sent = packet.sent(self.timer.elapsed());
        self.sent_list.push(sent);

        Ok(num_sent)
    }

    pub(crate) fn retry_ack_connection(&mut self, socket: &UdpSocket) -> AntarcResult<usize> {
        self.sequence_tracker = unsafe { Sequence::new_unchecked(1) };
        self.ack_connection(socket)
    }

    pub(crate) fn on_received_connection_acked_from_client(
        mut self,
        buffer: &[u8],
    ) -> Result<Host<Connected>, HostError<AcceptingConnection>> {
        let packet = unmove_on_error!(
            Packet::decode(buffer, self.timer.elapsed()),
            HostError,
            self
        );

        if let Header::DataTransfer(data_transfer_info) = &packet.header {
            self.ack_tracker = data_transfer_info.header_info.sequence.get();

            if self.on_receive_ack(&packet) {
                self.internals.push(packet.internald(self.timer.elapsed()));
                let connection_id = self.connection.0;

                let connected = self.into_new_state(Connected(connection_id));
                Ok(connected)
            } else {
                let error = Err(HostError {
                    err: format!(
                    "Received a packet {:#?} that does not ack the connection attempt for {:#?}.",
                    packet, self
                ),
                    unmoved: self,
                });
                error
            }
        } else {
            panic!(
                "{}",
                format!(
                    "Something went wrong when acking connection for {:#?}",
                    self
                )
            );
        }
    }
}
*/
