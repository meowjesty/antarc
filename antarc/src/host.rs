use std::{
    marker::PhantomData,
    net::{SocketAddr, UdpSocket},
    num::{NonZeroU16, NonZeroU32},
    time::{Duration, Instant},
};

use crate::{
    exponential_moving_average,
    packet::{
        Acked, Header, Internal, Packet, Received, Retrieved, Sent, ToSend, CONNECTION_ACCEPTED,
        CONNECTION_REQUEST,
    },
};

/// TODO(alex) 2021-01-29: Think of `Sessions / Channels` when wondering about connections, it helps
/// when trying to figure out how to keep alive a session (connection), how the communication
/// between hosts occur (channels trasnfer packets), and gives more struct names for similar things.
#[derive(Debug)]
pub struct Connecting;

#[derive(Debug)]
pub struct Connected;

#[derive(Debug)]
pub struct Disconnected;

#[derive(Debug)]
pub(crate) struct Host<ConnectionState> {
    pub(crate) address: SocketAddr,
    /// TODO(alex) 2021-02-26: Should each `Host` have its own `Instant` to keep track of timings?
    /// This is not a hard requirement, but many functions will end up using the `Instant` to
    /// calculate time related things, so it might as well be in the struct.
    pub(crate) timer: Instant,
    pub(crate) sequence: NonZeroU32,
    /// TODO(alex) 2021-02-13: Do not flood the network, find a way to check if the `rtt` is
    /// increasing due to us flooding the network with packets.
    pub(crate) rtt: u128,
    pub(crate) received_list: Vec<Packet<Received>>,
    pub(crate) retrieved: Vec<Packet<Retrieved>>,
    pub(crate) internals: Vec<Packet<Internal>>,
    pub(crate) send_queue: Vec<Packet<ToSend>>,
    pub(crate) sent_list: Vec<Packet<Sent>>,
    pub(crate) acked_list: Vec<Packet<Acked>>,
    _phantom: PhantomData<ConnectionState>,
}

impl<State> Host<State> {
    /// 1. Check if the address is expected (this function returns an `Error` if the `remote_addr`
    ///    doesn't match this host's address);
    ///    - the packet validity is checked in the `Packet<Received>::decode` function.
    /// 2. Decode `buffer` into a `Packet<Received>`;
    /// 3. Use the received packet `ack` to move a packet from `sent_list` into `acked_list`, if
    ///    there is a `Packet<Sent>` with `sent.sequence == received.ack`;
    ///
    /// TODO(alex) 2021-02-25: Use the `received.past_acks` to ack multiple `Packet<Sent>` that
    /// might still be lingering in the `sent_list`.
    ///
    /// 4. Push the `Packet<Received>` into the `received_list` for further handling, either by
    /// some internal protocol mechanism (hearbeat, connection estabilishment), or for the user to
    /// retrieve.
    ///
    /// TODO(alex) 2021-02-26: Should each `Host` have its own `Instant` to keep track of timings?
    /// This is not a hard requirement, but many functions will end up using the `Instant` to
    /// calculate time related things, so it might as well be in the struct.
    pub(crate) fn raw_on_receive(
        &mut self,
        remote_addr: &SocketAddr,
        buffer: &[u8],
    ) -> Result<(), String> {
        // TODO(alex) 2021-02-23: What should happen here?
        // 1. Check `host.received_list` for internal, connection related packets;
        //   - what happens if we received data transfers from this host before (when it was in a
        //     connected state, but lost connection)?
        // NOTE(alex) 2021-02-26: This could be done here or in the manager `Client` part, but it
        // would just end up as duplicated code between possible `Host` states.
        if *remote_addr != self.address {
            return Err(format!(
                "expected {:?}, but received {:?}",
                self.address, remote_addr
            ));
        }

        let packet = Packet::decode(&buffer, self.timer.elapsed())?;
        if let Some((index, _)) = self
            .sent_list
            .iter_mut()
            .enumerate()
            .find(|(_, sent)| packet.header.ack == sent.header.sequence.get())
        {
            let sent = self.sent_list.remove(index);
            let acked = sent.acked(self.timer.elapsed());

            let delta_rtt: Duration = acked.state.time_acked - acked.state.time_sent;
            self.rtt = exponential_moving_average(delta_rtt.as_millis(), self.rtt);

            self.acked_list.push(acked);
        }

        self.received_list.push(packet);

        Ok(())
    }

    /// Internal function that may only be called by one of the valid `Host<State>` where sending a
    /// packet makes sense (currently: `Host<Connecting>` and `Host<Connected>`).
    ///
    /// 1. Remove a `Packet<ToSend>` from the `send_queue`;
    /// 2. Encode the packet;
    /// 3. Use the `socket` actually send the packet (`[u8]` buffer) to this `Host::address`;
    /// 4. Change the `Packet<ToSend>` into `Packet<Sent>` and move it to `Host::sent_list`;
    ///
    /// TODO(alex) 2021-02-25: There may be an alternative to this by using `Trait`s, but this works
    /// for now.
    fn raw_send(&mut self, socket: &UdpSocket) -> Result<(), String> {
        if let Some(to_send) = self.send_queue.pop() {
            let to_send_raw = to_send.encode()?;
            let num_sent = socket
                .send_to(&to_send_raw.buffer, self.address)
                .map_err(|fail| fail.to_string())?;

            self.sent_list.push(to_send.sent(self.timer.elapsed()));
        }

        Ok(())
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
            sequence: unsafe { NonZeroU32::new_unchecked(1) },
            rtt: 0,
            received_list: Vec::with_capacity(32),
            retrieved: Vec::with_capacity(32),
            internals: Vec::with_capacity(32),
            send_queue: Vec::with_capacity(32),
            sent_list: Vec::with_capacity(32),
            acked_list: Vec::with_capacity(32),
            _phantom: PhantomData::default(),
        };

        host
    }

    pub(crate) fn request_connection(mut self, connection_id: NonZeroU16) -> Host<Connecting> {
        let connection_header = Header {
            connection_id,
            sequence: unsafe { NonZeroU32::new_unchecked(1) },
            ack: 0,
            past_acks: 0,
            kind: CONNECTION_REQUEST,
        };
        let connection_request =
            Packet::<ToSend>::new(connection_header, vec![0; 10], self.timer.elapsed());
        // TODO(alex) 2021-02-17: This should be marked as priority and reliable, we can't move on
        // until the connection is estabilished, and the user cannot be able to put packets into
        // the `send_queue` until the protocol is done handling it.
        self.send_queue.push(connection_request);

        let host = Host {
            address: self.address,
            timer: self.timer,
            sequence: self.sequence,
            rtt: self.rtt,
            received_list: self.received_list,
            retrieved: self.retrieved,
            internals: self.internals,
            send_queue: self.send_queue,
            sent_list: self.sent_list,
            acked_list: self.acked_list,
            _phantom: PhantomData::default(),
        };

        host
    }

    /// TODO(alex) 2021-02-23: This is the `server`-side of the `Disconnected -> Connecting`
    /// transformation.
    pub(crate) fn accept_connection(mut self, connection_id: NonZeroU16) -> Host<Connecting> {
        let connection_header = Header {
            connection_id,
            sequence: unsafe { NonZeroU32::new_unchecked(1) },
            ack: 0,
            past_acks: 0,
            kind: CONNECTION_ACCEPTED,
        };
        let connection_request =
            Packet::<ToSend>::new(connection_header, vec![0; 10], self.timer.elapsed());
        // TODO(alex) 2021-02-17: This should be marked as priority and reliable, we can't move on
        // until the connection is estabilished, and the user cannot be able to put packets into
        // the `send_queue` until the protocol is done handling it.
        self.send_queue.push(connection_request);

        let host = Host {
            address: self.address,
            timer: self.timer,
            sequence: self.sequence,
            rtt: self.rtt,
            received_list: self.received_list,
            retrieved: self.retrieved,
            internals: self.internals,
            send_queue: self.send_queue,
            sent_list: self.sent_list,
            acked_list: self.acked_list,
            _phantom: PhantomData::default(),
        };

        host
    }
}

impl Host<Connecting> {
    pub(crate) fn into_connected(mut self) -> Host<Connected> {
        let host = Host {
            address: self.address,
            timer: self.timer,
            sequence: self.sequence,
            rtt: self.rtt,
            received_list: self.received_list,
            retrieved: self.retrieved,
            internals: self.internals,
            send_queue: self.send_queue,
            sent_list: self.sent_list,
            acked_list: self.acked_list,
            _phantom: PhantomData::default(),
        };

        host
    }

    pub(crate) fn into_disconnected(mut self) -> Host<Disconnected> {
        let host = Host {
            address: self.address,
            timer: self.timer,
            sequence: self.sequence,
            rtt: self.rtt,
            received_list: self.received_list,
            retrieved: self.retrieved,
            internals: self.internals,
            send_queue: self.send_queue,
            sent_list: self.sent_list,
            acked_list: self.acked_list,
            _phantom: PhantomData::default(),
        };

        host
    }

    pub(crate) fn on_receive(
        &mut self,
        remote_addr: &SocketAddr,
        buffer: &[u8],
    ) -> Result<(), String> {
        self.raw_on_receive(&remote_addr, buffer)
    }

    pub(crate) fn send(&mut self, socket: &UdpSocket) -> Result<(), String> {
        self.raw_send(&socket)
    }
}

impl Host<Connected> {
    pub(crate) fn on_receive(
        &mut self,
        remote_addr: &SocketAddr,
        buffer: &[u8],
    ) -> Result<(), String> {
        self.raw_on_receive(&remote_addr, buffer)?;

        Ok(())
    }

    pub(crate) fn send(&mut self, socket: &UdpSocket) -> Result<(), String> {
        self.raw_send(&socket)
    }
}
