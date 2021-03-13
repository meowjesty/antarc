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
#[derive(Debug)]
pub(crate) struct Disconnected;

#[derive(Debug, Default)]
pub(crate) struct RequestingConnection {
    retries: u32,
}

#[derive(Debug, Default)]
pub(crate) struct FailedToSendConnectionRequest {
    retries: u32,
    error: String,
}

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

#[derive(Debug)]
pub(crate) struct Connected(ConnectionId);

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

impl Host<Disconnected> {
    /// TODO(alex) 2021-02-07: Is it possible to create a `Host` in any other state? Or should it
    /// always start in disconnected mode?
    pub(crate) fn new(address: SocketAddr) -> Host<Disconnected> {
        let state = Disconnected;
        let timer = Instant::now();

        let host = Host {
            address,
            timer,
            sequence_tracker: unsafe { NonZeroU32::new_unchecked(1) },
            ack_tracker: 0,
            past_acks_tracker: 0,
            rtt: Duration::new(0, 0),
            received_list: Vec::with_capacity(32),
            retrieved: Vec::with_capacity(32),
            internals: Vec::with_capacity(32),
            send_queue: VecDeque::with_capacity(32),
            priority_queue: VecDeque::with_capacity(32),
            sent_list: Vec::with_capacity(32),
            acked_list: Vec::with_capacity(32),
            connection: Disconnected,
        };

        host
    }

    /// TODO(alex) 2021-03-09: `Client`-side only function.
    /// I can move this into a new host state like `RequestingConnection`, just like the server
    /// `AckingConnection`.
    ///
    /// TODO(alex) 2021-03-09: This `move` on success or failure makes me feel like I'm digging my
    /// own grave, when I think about what the higher level code will have to do to handle these
    /// moves. It begs some serious consideration on whether this FSM pattern is usable, or if I
    /// should drop it in favor of enums.
    pub(crate) fn request_connection(
        mut self,
        socket: &UdpSocket,
    ) -> Result<Host<RequestingConnection>, Host<FailedToSendConnectionRequest>> {
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

        let raw_packet = packet.encode().map_err(|fail| {
            let failed_to_send_connection_request = FailedToSendConnectionRequest {
                retries: 0,
                error: fail,
            };
            let host = self.into_new_state(failed_to_send_connection_request);
            host
        })?;

        let num_sent = socket
            .send_to(&raw_packet.buffer, self.address)
            .map_err(|fail| {
                let failed_to_send_connection_request = FailedToSendConnectionRequest {
                    retries: 0,
                    error: fail.to_string(),
                };
                let host = self.into_new_state(failed_to_send_connection_request);
                host
            })?;
        assert!(num_sent > 0);
        self.sequence_tracker = unsafe { Sequence::new_unchecked(self.sequence_tracker.get() + 1) };

        let sent = packet.sent(self.timer.elapsed());
        self.sent_list.push(sent);

        let requesting_connection = self.into_new_state(RequestingConnection::default());
        Ok(requesting_connection)
    }

    pub(crate) fn on_received_connection_request(
        mut self,
        buffer: &[u8],
        connetion_id: ConnectionId,
    ) -> Result<Host<AcceptingConnection>, HostError<Disconnected>> {
        let packet = unmove_on_error!(
            Packet::decode(buffer, self.timer.elapsed()),
            HostError,
            self
        );

        unimplemented!()
        /*
        if let Header::ConnectionRequest(connection_request_info) = &packet.header {
            self.ack_tracker = connection_request_info.header_info.sequence.get();

            self.internals.push(packet.internald(self.timer.elapsed()));
            let acking_connection = self.into_new_state(AcceptingConnection(connetion_id));
            // TODO(alex) 2021-03-09: Ack the connection (send connection accepted).
            //
            // ADD(alex) 2021-03-11: This is the hard problem, we need to `poll` the socket to check
            // if it's ready to send, before trying to send stuff, this will require a polling
            // `loop`, otherwise we get out of this function and have to call it again.
            //
            // If we don't call `socket.send` in here, then it has to be called later by the outer
            // owner of this `Host`, which is kinda the same as the problem above. Having `socket`
            // stuff inside the `Host` implementation complicates things, but how do I avoid it?
            //
            // I thought about having an intermmediate state, like `RequestingConnection` (for the
            // `Client`), but I don't see the benefits, it ends up eating the same problem space as
            // the `AwaitingConnectionAck` state.
            //
            // Let's think about it (Client):
            // 1. client polls socket for sending connection request to `Disconnected`:
            //  1.a if the socket is ready, then send packet, host is now `AwaitingConnectionAck`;
            //  1.b otherwise, poll again;
            // 2. client checks for timeout (too long since request was sent):
            //  2.a if not long enough, then re-send the packet, and increment retries;
            //  2.b otherwise, go back to `Disconnected`;
            // 3. client receives an ack, and changes state to `Connected`;
            //
            // This shows that state changes may happen on either `send` or `receive`, it depends on
            // the state. Disconnectd -> AwaitingConnectionAck happens on **send**, meanwhile
            // AwaitingConnectionAck -> Connected on **receive**.
            //
            // For the `Server` is a bit of the opposite, where the first state transition occurs on
            // **receive**.
            //
            // The main theme I'm noticing here is that I need some form of _reactive_ state
            // transitions, when you receive this, then send this back, when you send that, then
            // you're in a different state now.
            //
            // ADD(alex) 2021-03-11: To make things work, let's start by not overthinking things, do
            // the `loop` wherever it's needed, ignore the future need for polling, if neccessary.
            //
            // ADD(alex) 2021-03-12: Let the `Client` handle failures, and have a `retry` or
            // whatever method, maybe just call the same method again.
            //
            // ADD(alex) 2021-03-13: Instead of returning to a previous state on error, we could
            // actually move into some error state, like
            // `Disconnected -> AwaitingConnectionAck | // FailedToSendConnectionRequest`. Thinking
            // about possible "branching" states might even help when dealing with these "where do
            // we poll" questions.
            Ok(acking_connection)
        } else {
            Err(HostError {
                err: format!(
                    "Received packet {:#?}, but expected `ConnectionRequest` for {:#?}",
                    packet, self
                ),
                unmoved: self,
            })
        }
        */
    }
}

impl Host<FailedToSendConnectionRequest> {
    /// TODO(alex) 2021-03-13: This is basically the same as the initial
    /// `Disconnected->RequestingConnection`, except it clears the host information.
    pub(crate) fn retry(
        mut self,
        socket: &UdpSocket,
    ) -> Result<Host<RequestingConnection>, Host<FailedToSendConnectionRequest>> {
        let disconnected = Host::new(self.address);
        disconnected.request_connection(socket)
    }
}

impl Host<FailedToReceiveConnectionAckInTime> {
    pub(crate) fn retry(
        mut self,
        socket: &UdpSocket,
    ) -> Result<Host<RequestingConnection>, Host<FailedToSendConnectionRequest>> {
        unimplemented!()
    }
}

impl Host<RequestingConnection> {
    /// TODO(alex) 2021-03-13: I think this function could be just some code in the `Client`, to
    /// avoid further complications here.
    pub(crate) fn poll(mut self) -> Result<Self, Host<FailedToReceiveConnectionAckInTime>> {
        assert!(self.sent_list.last().is_some());
        let last_sent = self.sent_list.last().unwrap();
        if last_sent.state.time_sent < self.timer.elapsed() - Duration::from_millis(1000) {
            let failed_to_receive_connection_ack_in_time = FailedToReceiveConnectionAckInTime {
                retries: self.connection.retries,
                error: format!("Failed to receive connection ack in time {:#?}.", self),
            };

            Err(self.into_new_state(failed_to_receive_connection_ack_in_time))
        } else {
            Ok(self)
        }
    }

    fn retry(mut self, socket: &UdpSocket) -> Result<Host<Connected>, Host<FailedToConnect>> {
        todo!()
    }

    pub(crate) fn on_received_connection_ack(
        mut self,
        buffer: &[u8],
    ) -> Result<Host<Connected>, Host<FailedToConnect>> {
        let packet = Packet::decode(buffer, self.timer.elapsed()).map_err(|fail| {
            let failed_to_connect = FailedToConnect {
                error: format!("Failed to decode connection packet {:#?}.", self),
            };

            self.into_new_state(failed_to_connect)
        })?;

        if let Header::ConnectionAccepted(connection_accepted_info) = &packet.header {
            if self.on_receive_ack(&packet) {
                self.internals.push(packet.internald(self.timer.elapsed()));

                let connected =
                    self.into_new_state(Connected(connection_accepted_info.connection_id));
                Ok(connected)
            } else {
                let failed_to_connect = FailedToConnect {
                    error: format!("Failed to connect to {:#?}.", self),
                };

                Err(self.into_new_state(failed_to_connect))
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

impl Host<FailedToConnect> {
    pub(crate) fn retry(
        mut self,
        socket: &UdpSocket,
    ) -> Result<Host<RequestingConnection>, Host<Disconnected>> {
        let requesting_connection = RequestingConnection { retries: 1 };
        let requesting_connection = self.into_new_state(requesting_connection);
        requesting_connection.retry(socket)

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
    }
}

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

impl Host<Connected> {
    /// TODO(alex) 2021-03-04: Having a `receive` in the `Host` complicates things too much, we end
    /// up having to do state transformations, so can't take `self` reference. It's easier to do it
    /// on the `Client`.
    pub(crate) fn receive(&mut self, buffer: &[u8]) -> AntarcResult<Host<Connected>> {
        unimplemented!()
    }
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
            let Connected(connection_id) = self.connection;
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
