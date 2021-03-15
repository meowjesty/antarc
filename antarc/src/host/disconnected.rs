use std::{
    collections::VecDeque,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    num::NonZeroU32,
    time::{Duration, Instant},
};

use super::requesting_connection::RequestingConnection;
use crate::{
    host::{ConnectionId, Host},
    packet::{
        header::{ConnectionRequestInfo, Header, HeaderInfo},
        to_send::ToSend,
        Packet, Payload, Sequence, CONNECTION_REQUEST,
    },
};

#[derive(Debug)]
enum DisconnectedStates {
    RequestingConnection(Host<RequestingConnection>),
    FailedToRequestConnection(Host<FailedToRequestConnection>),
}

#[derive(Debug)]
pub(crate) struct Disconnected;

#[derive(Debug, Default)]
pub(crate) struct FailedToRequestConnection {
    attempts: u32,
    error: String,
}

impl Default for Host<Disconnected> {
    fn default() -> Self {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 7777);
        let connection = Disconnected;
        let timer = Instant::now();

        let host = Host {
            address,
            timer,
            sequence_tracker: unsafe { Sequence::new_unchecked(1) },
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
            connection,
        };

        host
    }
}

impl Host<Disconnected> {
    /// TODO(alex) 2021-02-07: Is it possible to create a `Host` in any other state? Or should it
    /// always start in disconnected mode?
    pub(crate) fn new(address: SocketAddr) -> Host<Disconnected> {
        let timer = Instant::now();

        let host = Host {
            address,
            timer,
            ..Default::default()
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
    pub(crate) fn request_connection(mut self, socket: &UdpSocket) -> DisconnectedStates {
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

                let requesting_connection = self.into_new_state(RequestingConnection::default());
                DisconnectedStates::RequestingConnection(requesting_connection)
            }
            Err(fail) => {
                let failed_to_send_connection_request = FailedToRequestConnection {
                    attempts: 1,
                    error: fail.to_string(),
                };
                let host = self.into_new_state(failed_to_send_connection_request);
                DisconnectedStates::FailedToRequestConnection(host)
            }
        }
    }
}

impl Host<FailedToRequestConnection> {
    /// TODO(alex) 2021-03-13: This is basically the same as the initial
    /// `Disconnected->RequestingConnection`, except it clears the host information.
    pub(crate) fn retry(
        mut self,
        socket: &UdpSocket,
    ) -> Result<Host<RequestingConnection>, Host<FailedToRequestConnection>> {
        let disconnected = Host::new(self.address);
        disconnected.request_connection(socket)
    }
}

// pub(crate) fn on_received_connection_request(
//     mut self,
//     buffer: &[u8],
//     connetion_id: ConnectionId,
// ) -> Result<Host<AcceptingConnection>, HostError<Disconnected>> {
//     let packet = unmove_on_error!(
//         Packet::decode(buffer, self.timer.elapsed()),
//         HostError,
//         self
//     );

// unimplemented!()
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
// }
