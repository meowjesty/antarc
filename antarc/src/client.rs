use std::{
    net::{SocketAddr, UdpSocket},
    time::{Duration, Instant},
};

use hecs::World;

use crate::{
    host::{Address, Connected, Disconnected, RequestingConnection},
    net::NetManager,
    packet::{
        header::Header, ConnectionId, ConnectionRequest, DataTransfer, Footer, Payload, Received,
        Retrieved, ToSend,
    },
    receiver::Source,
    sender::Destination,
    MTU_LENGTH,
};

/// TODO(alex) 2021-02-26: References for ideas about connection:
/// http://www.tcpipguide.com/free/t_PPPLinkSetupandPhases.htm
///
/// ADD(alex) 2021-02-26: We need a `Disconnecting` state (link termination phase)?
///
/// TODO(alex) 2021-03-04: Client and Server are different beasts right now, I'm thinking about
/// ways of allowing some sort of peer-to-peer communication, so a `Client` would have to track
/// connection (`Host<State>`) for multiple other clients. To to this we would need something
/// that looks more like the `Server`, and some way to keep one node of the network as the main
/// server? This idea is not clear yet.
pub struct Client {}

impl NetManager<Client> {
    pub fn new_client(address: &SocketAddr) -> Self {
        let socket = UdpSocket::bind(address).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(1000)))
            .unwrap();

        let timer = Instant::now();

        let world = World::new();

        let client = Client {};

        let buffer = vec![0x0; MTU_LENGTH];

        NetManager {
            world,
            socket,
            timer,
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
    pub fn connect(&mut self, server_addr: &SocketAddr) -> () {
        let world = &mut self.world;

        // TODO(alex) 2021-04-03: Check for existing hosts in different states, we do nothing
        // if the host is already `RequestingConnection, AwaitingConnectionAck, Connected`, we only
        // want to do something if `Disconnected` or non-existent.
        //
        // ADD(alex) 2021-04-03: This check is probably good enough.
        let existing_host_id = world
            .query::<(&Address,)>()
            .with::<Disconnected>()
            .iter()
            .find_map(|(host_id, (address,))| (address.0 == *server_addr).then_some(host_id));

        // NOTE(alex) 2021-04-04: Can't combine this in the `find_map` because `query` does a
        // mutable borrow of `world`, so it doesn't allow `world.spawn`.
        let host_id = existing_host_id.unwrap_or_else(|| {
            let host_id = world.spawn((
                Address(server_addr.clone()),
                RequestingConnection { attempts: 0 },
            ));
            host_id
        });

        let connection_request_header = Header::default();
        let _packet_id = world.spawn((
            connection_request_header,
            Address(server_addr.clone()),
            ToSend {
                time: self.timer.elapsed(),
            },
            ConnectionRequest,
            Destination { host_id },
        ));
    }

    pub fn connected(&mut self) {}

    pub fn denied(&mut self) {
        todo!()
    }

    /// TODO(alex) 2021-02-23: Return some indication that the manager received new packets and the
    /// user should call `retrieve`.
    /// ADD(alex) 2021-02-26: The return can be made even more general, by having an enum of
    /// possibilities, like `HasMessagesToRetrieve`, `ConnectionLost`. The only success cases I can
    /// think of are `HasMessagesToRetrieve` and `NothingToReport`? But the errors are plenty, like
    /// `ReceivingMessageFromBannedHost`, `FailedToSend`, `FailedToReceive`, `FailedToEncode`, ...
    pub fn poll(&mut self) -> () {
        todo!()
    }

    /// TODO(alex) 2021-03-07: Think of how network libraries usually have a `listen` function,
    /// instead of manually calling `receive`.
    /// NOTE(alex): This is less of a system, and more just a function that the user will call, part
    /// of the public API (exposed via `NetManager` client / server).
    pub fn retrieve(&mut self) -> Vec<(ConnectionId, Vec<u8>)> {
        let world = &mut self.world;

        let mut data_transfers = Vec::with_capacity(8);
        let mut result = Vec::with_capacity(8);
        let mut invalid_packets = Vec::with_capacity(8);

        for (packet_id, (header, payload, footer, source, received)) in world
            .query::<(&Header, &Payload, &Footer, &Source, &Received)>()
            .with::<DataTransfer>()
            .without::<Retrieved>()
            .iter()
        {
            let host_query = world.query_one::<&Address>(source.host_id).unwrap();
            // NOTE(alex) 2021-04-09: This is the main difference between `Client` and `Server`, as
            // the client will only accept `DataTransfer`s after the connection is estabilished, but
            // the server will receive a `DataTransfer` while the `Host` is still in the
            // `AwaitingAck`-sort of state, this packet will ack the `ConnectionAccepted` packet
            // that the server sent previously, and estabilish the connection.
            if let Some(_) = host_query.with::<Connected>().get() {
                data_transfers.push(packet_id);
                result.push((footer.connection_id.unwrap(), payload.0.clone()));
            } else {
                invalid_packets.push(packet_id);
            }
        }

        while let Some(packet_id) = data_transfers.pop() {
            let _ = world
                .insert(
                    packet_id,
                    (Retrieved {
                        time: self.timer.elapsed(),
                    },),
                )
                .unwrap();
        }

        while let Some(packet_id) = invalid_packets.pop() {
            let _ = world.despawn(packet_id).unwrap();
        }

        result
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
    pub fn send(&mut self, data: Vec<u8>) -> () {
        todo!()
    }

    pub fn send_priority(&self, data: Vec<u8>) -> () {
        todo!();
    }
}
