use std::{
    iter,
    net::SocketAddr,
    num::{NonZeroU16, NonZeroU32},
    time::{Duration, Instant},
};

use async_std::{future, net::UdpSocket};

use crate::{
    host::{Connected, Disconnected, Host, CONNECTION_TIMEOUT_THRESHOLD},
    net::NetManager,
    packet::{ConnectionId, DataTransferInfo, Header, Packet, Payload, Sequence, DATA_TRANSFER},
    AntarcResult, MTU_LENGTH,
};

/// TODO(alex) 2021-02-26: References for ideas about connection:
/// http://www.tcpipguide.com/free/t_PPPLinkSetupandPhases.htm
/// ADD(alex) 2021-02-26: We need a `Disconnecting` state (link termination phase)?
#[derive(Debug)]
pub(crate) enum Connection {
    Disconnected(Host<Disconnected>),
    /// TODO(alex) 2021-03-04: Prevent flooding of connecting state attack.
    Connecting(Host<Connecting>),
    Connected(Host<Connected>),
}

/// TODO(alex) 2021-03-04: Client and Server are different beasts right now, I'm thinking about
/// ways of allowing some sort of peer-to-peer communication, so a `Client` would have to track
/// connection (`Host<State>`) for multiple other clients. To to this we would need something
/// that looks more like the `Server`, and some way to keep one node of the network as the main
/// server? This idea is not clear yet.
#[derive(Debug)]
pub struct Client {
    other_clients: Vec<Host<Disconnected>>,
    connection: Option<Connection>,
}

impl NetManager<Client> {
    pub async fn new_client(address: &SocketAddr) -> Self {
        let socket = UdpSocket::bind(address).await.unwrap();

        let client = Client {
            other_clients: Vec::with_capacity(8),
            connection: None,
        };

        let buffer = vec![0x0; MTU_LENGTH];

        NetManager {
            socket,
            buffer,
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
    pub async fn connect(&mut self, server_addr: &SocketAddr) -> AntarcResult<()> {
        let client = &mut self.client_or_server;

        // TODO(alex) 2021-02-23: Do I want to always `replace` or if the user calls `connect`
        // twice should it check if there is a `Host<Disconnected>` already and do something else?
        let mut disconnected = Host::new(*server_addr);
        let _ = disconnected.request_connection(&self.socket).await?;
        let mut connecting = disconnected.connecting();

        let (size_recv, remote_addr) = loop {
            break match future::timeout(
                Duration::from_millis(1000),
                self.socket.recv_from(&mut self.buffer),
            )
            .await
            {
                Ok(result) => result,
                Err(timed_out) => {
                    if let Some(last_sent) = connecting.sent_list.last() {
                        if last_sent.state.time_sent > CONNECTION_TIMEOUT_THRESHOLD {
                            connecting.retry(&self.socket).await?;
                        }
                    }
                    continue;
                }
            }
            .map_err(|fail| fail.to_string())?;
        };

        if remote_addr != connecting.address {
            return Err(format!(
                "Expected packet from {:?}, but received from {:?}",
                connecting.address, remote_addr
            ));
        }

        let packet = connecting.on_receive(&self.buffer[..size_recv])?;
        if connecting
            .acked_list
            .iter()
            .any(|acked| acked.header.get_sequence().get() == packet.header.get_ack())
        {
            match &packet.header {
                Header::ConnectionAccepted(accepted_info) => {
                    let mut connected = connecting.connection_accepted(accepted_info.connection_id);
                    connected.on_internal(packet);
                    client.connection = Some(Connection::Connected(connected));

                    return Ok(());
                }
                Header::ConnectionDenied(denied_info) => {
                    let fail = Err(format!("Connection failed {:?}.", denied_info));

                    let mut disconnected = connecting.connection_denied();
                    disconnected.on_internal(packet);
                    client.connection = Some(Connection::Disconnected(disconnected));

                    return fail;
                }
                fail_header => {
                    return Err(format!("Unexpected `Header` type of {:?}.", fail_header))
                }
            }
        } else {
            // TODO(alex) 2021-03-08: Do we need to loop here, what are the potential causes of
            // getting in here?
            let mut disconnected = connecting.connection_denied();
            disconnected.on_internal(packet);
            client.connection = Some(Connection::Disconnected(disconnected));
            Err(format!(
                "Connection packet not acked, aborting connection for {:?}.",
                self
            ))
        }
    }

    pub fn connected(&mut self) {
        if let Some(Connection::Connecting(connecting)) = self.client_or_server.connection.take() {
            todo!();
        }
    }

    pub fn denied(&mut self) {
        let client = &mut self.client_or_server;

        if let Some(Connection::Connecting(connecting)) = client.connection.take() {
            let disconnected = connecting.connection_denied();
            client.connection = Some(Connection::Disconnected(disconnected));
            todo!()
        }
    }

    /// TODO(alex) 2021-02-23: Return some indication that the manager received new packets and the
    /// user should call `retrieve`.
    /// ADD(alex) 2021-02-26: The return can be made even more general, by having an enum of
    /// possibilities, like `HasMessagesToRetrieve`, `ConnectionLost`. The only success cases I can
    /// think of are `HasMessagesToRetrieve` and `NothingToReport`? But the errors are plenty, like
    /// `ReceivingMessageFromBannedHost`, `FailedToSend`, `FailedToReceive`, `FailedToEncode`, ...
    pub async fn poll(&mut self) -> AntarcResult<bool> {
        let client = &mut self.client_or_server;
        let mut may_retrieve = false;

        let (size, remote_addr) = self
            .socket
            .recv_from(&mut self.buffer)
            .await
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
                    // self.connect(&disconnected.address)?;
                }
                Connection::Connecting(mut connecting) => {
                    connecting.on_receive(&self.buffer)?;
                    if connecting.rtt > CONNECTION_TIMEOUT_THRESHOLD {
                        connection = Connection::Disconnected(connecting.connection_denied());
                        client.connection = Some(connection);

                        return Err(format!("Connection timeout for {:?}.", client.connection));
                    }

                    // TODO(alex) 2021-02-23:  what happens if we received data transfers from this
                    // host before (when it was in a connected state, but lost connection)?
                    // let _ = connecting.on_receive(&remote_addr, &self.buffer)?;

                    if let Some(received) = connecting.received_list.pop() {
                        match &received.header {
                            Header::ConnectionRequest(connection_request_info) => {
                                if connecting.acked_list.iter().any(|acked| {
                                    acked.header.get_sequence().get()
                                        == connection_request_info.header_info.ack
                                }) {
                                    let internald = received.internald(connecting.timer.elapsed());
                                    connecting.internals.push(internald);
                                    self.connected();
                                } else {
                                    // TODO(alex) 2021-03-04: Noone acked our connection request
                                    // packet yet, so we must resend the packet. The connection
                                    // request has to be reliable, so we need a way to not increment
                                    // its sequence value, as this could make connecting more
                                    // complex than it has to be.
                                }
                            }
                            Header::ConnectionDenied(connection_denied_info) => {
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
                        // let _ = connecting.send(&self.socket)?;
                    }
                }
                Connection::Connected(mut connected) => {
                    // let _ = connected.on_receive(&remote_addr, &self.buffer)?;
                    // TODO(alex) 2021-02-27: Handle the data transfer + hearbeat here.
                    let _ = connected.send(&self.socket).await?;
                }
            }
        }

        Ok(may_retrieve)
    }

    /// TODO(alex) 2021-03-07: Think of how network libraries usually have a `listen` function,
    /// instead of manually calling `receive`.
    pub async fn receive(&mut self) -> AntarcResult<()> {
        let client = &mut self.client_or_server;

        let (num_recv, remote_addr) = self
            .socket
            .recv_from(&mut self.buffer)
            .await
            .map_err(|fail| fail.to_string())?;

        if let Some(connection) = client.connection.as_mut() {
            match connection {
                Connection::Connected(connected) => {
                    if remote_addr != connected.address {
                        Err(format!(
                            "Expected packet from {:?}, but received from {:?}",
                            connected.address, remote_addr
                        ))
                    } else {
                        connected.on_receive(&self.buffer)
                    }
                }
                Connection::Connecting(connecting) => {
                    if remote_addr != connecting.address {
                        Err(format!(
                            "Expected packet from {:?}, but received from {:?}",
                            connecting.address, remote_addr
                        ))
                    } else {
                        todo!()
                        // connecting.on_receive(&self.buffer)
                    }
                }
                fail_state => {
                    panic!(
                        "{}",
                        format!(
                            "Received a packet from a host in an unexpected state {:?}.",
                            fail_state
                        )
                    )
                }
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
    pub async fn send(&mut self, data: Vec<u8>) -> AntarcResult<Sequence> {
        let client = &mut self.client_or_server;
        if let Some(connected) = client.connection.as_mut() {
            match connected {
                Connection::Connected(host) => {
                    // TODO(alex) 2021-03-03: Enqueue here is temporary, when `Host::send` is async
                    // this should probably be there.
                    host.send_queue.push_back(Payload(data));
                    let sent = host.send(&self.socket).await?;

                    return Ok(host.sequence_tracker);
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
