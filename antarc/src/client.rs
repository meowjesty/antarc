use std::{
    net::{SocketAddr, UdpSocket},
    num::{NonZeroU16, NonZeroU32},
    time::Instant,
};

use crate::{
    host::{Connected, Connecting, Disconnected, Host, CONNECTION_TIMEOUT_THRESHOLD},
    net::NetManager,
    packet::{ConnectionId, DataTransferInfo, Header, Packet, Payload, Sequence, DATA_TRANSFER},
    AntarcResult,
};

/// TODO(alex) 2021-02-26: References for ideas about connection:
/// http://www.tcpipguide.com/free/t_PPPLinkSetupandPhases.htm
/// ADD(alex) 2021-02-26: We need a `Disconnecting` state (link termination phase)?
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
/// ADD(alex) 2021-02-25: I'm questioning this, it might belong here after all.
impl NetManager<Client> {
    pub fn new_client(address: &SocketAddr) -> Self {
        let socket = UdpSocket::bind(address).unwrap();

        let client = Client {
            other_clients: Vec::with_capacity(8),
            connection: None,
        };

        // TODO(alex) 2021-02-26: Each `Host` will probably have it's own `buffer`, like the
        // `timer`.
        let buffer = vec![0x0; 1024];

        NetManager {
            socket,
            buffer,
            connection_id_tracker: unsafe { NonZeroU16::new_unchecked(1) },
            client_or_server: client,
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
    ///
    /// TODO(alex) 2021-02-26: Authentication is something that we can't do here, it's up to the
    /// user, but there must be an API for forcefully dropping a connection, plus banning a host.
    pub fn connect(&mut self, server_addr: &SocketAddr) -> AntarcResult<()> {
        let disconnected = Connection::Disconnected(Host::new(*server_addr));

        // TODO(alex) 2021-02-23: Do I want to always `replace` or if the user calls `connect`
        // twice should it check if there is a connection already and do something else?
        if let Some(Connection::Disconnected(disconnected)) = self
            .client_or_server
            .connection
            .replace(disconnected)
            .take()
        {
            let mut connecting = disconnected.request_connection(self.connection_id_tracker);
            connecting.send(&self.socket)?;

            self.client_or_server.connection = Some(Connection::Connecting(connecting));
        }

        Err(format!("Connection not implemented yet properly!"))
    }

    pub fn connected(&mut self) {
        if let Some(Connection::Connecting(connecting)) = self.client_or_server.connection.take() {
            let connected = connecting.into_connected(ConnectionId::new(1).unwrap());
            self.client_or_server.connection = Some(Connection::Connected(connected));
            todo!();
        }
    }

    pub fn denied(&mut self) {
        let client = &mut self.client_or_server;

        if let Some(Connection::Connecting(connecting)) = client.connection.take() {
            let disconnected = connecting.into_disconnected();
            client.connection = Some(Connection::Disconnected(disconnected));
        }
    }

    /// TODO(alex) 2021-02-23: Return some indication that the manager received new packets and the
    /// user should call `retrieve`.
    /// ADD(alex) 2021-02-26: The return can be made even more general, by having an enum of
    /// possibilities, like `HasMessagesToRetrieve`, `ConnectionLost`. The only success cases I can
    /// think of are `HasMessagesToRetrieve` and `NothingToReport`? But the errors are plenty, like
    /// `ReceivingMessageFromBannedHost`, `FailedToSend`, `FailedToReceive`, `FailedToEncode`, ...
    pub fn poll(&mut self) -> AntarcResult<bool> {
        let client = &mut self.client_or_server;
        let mut may_retrieve = false;

        let (size, remote_addr) = self
            .socket
            .recv_from(&mut self.buffer)
            .map_err(|fail| fail.to_string())?;

        if let Some(mut connection) = client.connection.take() {
            // TODO(alex) 2021-02-23: What should happen here?
            // 4. Ack packet(s) from `sent` list with the `packet.past_acks` value received;
            //   - this is done by checking the last `16` packets based on the received `ack`;
            // 8. If last packet sent time > live connection threshold, drop the connection;
            //
            match connection {
                Connection::Disconnected(disconnected) => {
                    // TODO(alex) 2021-02-23: We enter this state when the connection has been lost
                    // for whatever reason, then try to reconnect here. Should we keep the host
                    // information, or just discard (current behavior is discard)?
                    dbg!("Attempting to reconnect to {:?}", &disconnected);
                    self.connect(&disconnected.address);
                }
                Connection::Connecting(mut connecting) => {
                    if connecting.rtt > CONNECTION_TIMEOUT_THRESHOLD {
                        connection = Connection::Disconnected(connecting.into_disconnected());
                        return Err("Connection timeout.".to_string());
                    }

                    // TODO(alex) 2021-02-23:  what happens if we received data transfers from this
                    // host before (when it was in a connected state, but lost connection)?
                    // let _ = connecting.on_receive(&remote_addr, &self.buffer)?;

                    if let Some(received) = connecting.received_list.last() {
                        match &received.header {
                            Header::ConnectionRequest(request) => {
                                let received = connecting.received_list.pop().unwrap();
                                let internald = received.internald(connecting.timer.elapsed());
                                connecting.internals.push(internald);
                                self.connected();
                            }
                            Header::ConnectionDenied(denied) => {
                                let received = connecting.received_list.pop().unwrap();
                                let internald = received.internald(connecting.timer.elapsed());
                                connecting.internals.push(internald);
                                self.denied();
                            }
                            invalid => {
                                let error = format!(
                                    "{:?} expected either a connection accepted \
                                    or denied, but got {:?} instead.",
                                    connecting, invalid
                                );
                                return Err(error);
                            }
                        }
                    } else {
                        let _ = connecting.send(&self.socket)?;
                    }
                }
                Connection::Connected(mut connected) => {
                    // let _ = connected.on_receive(&remote_addr, &self.buffer)?;
                    // TODO(alex) 2021-02-27: Handle the data transfer + hearbeat here.
                    let _ = connected.send(&self.socket)?;
                }
            }
        }

        Ok(may_retrieve)
    }

    pub fn receive(&mut self) -> AntarcResult<()> {
        let client = &mut self.client_or_server;
        if let Some(connected) = self.client_or_server.connection.as_mut() {
            match connected {
                Connection::Connected(host) => {
                    let (num_recv, remote_addr) = self
                        .socket
                        .recv_from(&mut self.buffer)
                        .map_err(|fail| fail.to_string())?;

                    if remote_addr != host.address {
                        Err(format!(
                            "Expected packet from {:?}, but received from {:?}",
                            host.address, remote_addr
                        ))
                    } else {
                        host.receive(&self.buffer)
                    }
                }
                fail_state => panic!(
                    "{}",
                    format!(
                        "Tried to enqueue a data transfer in a non-connected client {:?}.",
                        fail_state
                    )
                ),
            }
        } else {
            Err(format!(
                "`Client::receive` called on non-connected client {:?}.",
                self
            ))
        }
    }

    /// TODO(alex) 2021-02-28: Returns the `Packet<ToSend>::Header::sequence` value so that the user
    /// may use it to remove the packet (if wanted). It'll be an API for users that might want to
    /// clear older packets that were never sent, and to allow users to check for packets that were
    /// actually sent.
    ///
    /// ADD(alex) 2021-03-03: This function is `async` when I think about it. The whole idea of
    /// queueing the packets to send, instead of sending them directly comes from the need to check
    /// if the `socket` is writable, so we enqueue the packets instead of just calling
    /// `socket.send` here, creating an async version of `socket.send` basically. `Client::tick`
    /// is very similar to what I think `poll` would look like.
    ///
    /// Using `Future` would simplify the API, as it would remove the need for the `Client::tick`
    /// function, so `Client::connect -> Future<ConnectedClient>` would do the connection handling
    /// that `Client::tick` is doing, while `Client::send` and `Client::receive` would do the rest
    /// (data transfer handling). `Client::receive` acts like the "update" function, as it will be
    /// called all the time (with some user tickrate), being equivalent to a "listen" function.
    pub fn send(&mut self, data: Vec<u8>) -> AntarcResult<Sequence> {
        let client = &mut self.client_or_server;
        if let Some(connected) = client.connection.as_mut() {
            match connected {
                Connection::Connected(host) => {
                    // TODO(alex) 2021-03-03: Enqueue here is temporary, when `Host::send` is async
                    // this should probably be there.
                    host.send_queue.push_back(Payload(data));
                    let sent = host.send(&self.socket)?;

                    return Ok(host.sequence);
                }
                fail_state => panic!(
                    "{}",
                    format!(
                        "Tried to enqueue a data transfer in a non-connected client {:?}.",
                        fail_state
                    )
                ),
            };
        }

        Err(format!("Send failed for client {:?}.", self))
    }

    pub fn send_priority(&self, data: Vec<u8>) -> Sequence {
        todo!();
    }
}
