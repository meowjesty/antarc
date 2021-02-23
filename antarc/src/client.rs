use std::{
    cell::Cell,
    net::{SocketAddr, UdpSocket},
    num::{NonZeroU16, NonZeroU32},
};

use crate::{
    host::{Connected, Connecting, Disconnected, Host},
    packet::Packet,
    peer::NetManager,
};

#[derive(Debug)]
pub(crate) enum Connection {
    Disconnected(Host<Disconnected>),
    Connecting(Host<Connecting>),
    Connected(Host<Connected>),
}

/// TODO(alex) 2021-02-22: Change this to `Box<Host<Disconnected>>, ...` to mimick the `Server`
/// struct. The approach of `enum Connection` will work, but its benefits are hard to notice (if
/// they even exist). Substitute `Client` with `ClientBox` (keep the name `Client`).
#[derive(Debug)]
pub struct Client {
    other_clients: Vec<Host<Disconnected>>,
    connection: Option<Connection>,
}

/// TODO(alex) 2021-02-14: This should be in the `net` crate, I want to avoid having sockets
/// integrated into the lower parts of the protocol. It should handle the state transitions for
/// packets, and connections, but leave the actual send/receive to the `net` crate.
impl NetManager<Client> {
    pub fn new_client(address: &SocketAddr) -> Self {
        let socket = UdpSocket::bind(address).unwrap();

        let client = Client {
            other_clients: Vec::with_capacity(8),
            connection: None,
        };

        let buffer = vec![0x0; 1024];

        NetManager {
            socket,
            buffer,
            connection_id_tracker: unsafe { NonZeroU16::new_unchecked(1) },
            kind: client,
        }
    }

    /// Creates a new `Host<Connecting>` or updates an existing `Host<Disconnected>`, then adds a
    /// connection request packet to this host's `to_send` list.
    /// TODO(alex) 2021-02-07: Handle the connection request to a server, there are a few points
    /// to consider:
    ///
    /// 1. Is it okay to connect to another client, when this one is already connected to a server?
    ///    - probably not, keep connected checks to empty or single element in the client;
    /// 2. Create connection request packet:
    ///    - must check if there is an outgoing request already, that has not yet been acked.
    ///    - this function should keep itself to packet handling only, do not try to escalate it
    ///    into handling packet loss or anything like that, this will be part of the network
    ///    implementation, as it requires reading incoming packets.
    pub fn connect(&mut self, server_addr: &SocketAddr) {
        let disconnected = Connection::Disconnected(Host::new(*server_addr));

        // TODO(alex) 2021-02-23: Do I want to always `replace` or if the user calls `connect`
        // twice should it check if there is a connection already and do something else?
        if let Some(Connection::Disconnected(disconnected)) =
            self.kind.connection.replace(disconnected).take()
        {
            // TODO(alex): 2021-02-23: `request_connection` should probably be here and not on
            // `Host`, as the `Server` and `Client` are the ones interested in this.
            let connecting = disconnected.request_connection(self.connection_id_tracker);
            self.kind.connection = Some(Connection::Connecting(connecting));

            // TODO(alex) 2021-02-17: This is a 2-part problem:
            // 1. Here we need to check if we're wrapping the `u16::MAX` value and getting a zero
            // back, if so, then we need to clean the `connection_id` sequencer from
            // old values, find an unused value for it by searching through `disconnected` hosts;
            self.connection_id_tracker =
                NonZeroU16::new(self.connection_id_tracker.get() + 1).unwrap();
        }
    }

    pub fn connected(&mut self) {
        if let Some(Connection::Connecting(connecting)) = self.kind.connection.take() {
            let connected = connecting.into_connected();
            self.kind.connection = Some(Connection::Connected(connected));
        }
    }

    /// TODO(alex) 2021-02-23: Return some indication that the manager received new packets and the
    /// user should call `retrieve`.
    pub fn tick(&mut self) -> Result<bool, String> {
        let mut may_retrieve = false;
        let (size, from_addr) = self
            .socket
            .recv_from(&mut self.buffer)
            .map_err(|fail| fail.to_string())?;

        if let Some(connection) = self.kind.connection.take() {
            // TODO(alex) 2021-02-23: What should happen here?
            // 4. Ack packet(s) from `sent` list with the `packet.past_acks` value received;
            //   - this is done by checking the last `16` packets based on the received `ack`;
            // 5. Calculate `rtt` based on how long it took from sending the packet to its ack;
            // 8. If last packet sent time > live connection threshold, drop the connection;
            //
            // ADD(alex) 2021-02-23: The code inside here will be mostly the same in each branch,
            // it probably makes sense to move into `Host<T>` and just pass in whatever is needed
            // (the packet and `from_addr`), it's lacking a basic `handle_received` function, and a
            // `handle_send`.
            match connection {
                Connection::Disconnected(disconnected) => {
                    // TODO(alex) 2021-02-23: We enter this state when the connection has been lost
                    // for whatever reason, then try to reconnect here. Should we keep the host
                    // information, or just discard (current behavior is discard)?
                    dbg!("Attempting to reconnect to {:?}", &disconnected);
                    self.connect(&disconnected.address);
                }
                Connection::Connecting(mut connecting) => {
                    // TODO(alex) 2021-02-23: What should happen here?
                    // 1. Check `host.received_list` for internal, connection related packets;
                    //   - what happens if we received data transfers from this host before (when it
                    //     was in a connected state, but lost connection)?
                    if from_addr != connecting.address {
                        return Err(format!(
                            "expected {:?}, but received {:?}",
                            connecting.address, from_addr
                        ));
                    }

                    let packet = Packet::decode(&self.buffer)?;
                    if let Some((index, _)) = connecting
                        .sent
                        .iter_mut()
                        .enumerate()
                        .find(|(_, sent)| packet.header.ack == sent.header.sequence.get())
                    {
                        let sent = connecting.sent.remove(index);
                        connecting.acked.push(sent.acked());
                    }

                    connecting.received_list.push(packet);

                    if let Some(to_send) = connecting.send_queue.pop() {
                        let to_send_raw = to_send.encode()?;
                        let num_sent = self
                            .socket
                            .send_to(&to_send_raw.buffer, connecting.address)
                            .map_err(|fail| fail.to_string())?;

                        connecting.sent.push(to_send.sent());
                    }
                }
                Connection::Connected(mut connected) => {
                    if from_addr != connected.address {
                        return Err(format!(
                            "expected {:?}, but received {:?}",
                            connected.address, from_addr
                        ));
                    }

                    let packet = Packet::decode(&self.buffer)?;
                    if let Some((index, _)) = connected
                        .sent
                        .iter_mut()
                        .enumerate()
                        .find(|(_, sent)| packet.header.ack == sent.header.sequence.get())
                    {
                        let sent = connected.sent.remove(index);
                        connected.acked.push(sent.acked());
                    }

                    connected.received_list.push(packet);

                    if let Some(to_send) = connected.send_queue.pop() {
                        let to_send_raw = to_send.encode()?;
                        let num_sent = self
                            .socket
                            .send_to(&to_send_raw.buffer, connected.address)
                            .map_err(|fail| fail.to_string())?;

                        connected.sent.push(to_send.sent());
                    }
                }
            }
        }

        Ok(may_retrieve)
    }

    pub fn retrieve(&self) -> Vec<(u32, Vec<u8>)> {
        todo!();
    }

    pub fn enqueue(&self, data: Vec<u8>) {
        todo!();
    }
}
