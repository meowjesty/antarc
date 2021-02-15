use std::net::{SocketAddr, UdpSocket};

use antarc::peer::{Client, Peer, Server};

fn client_main() {
    let server_addr: SocketAddr = "127.0.0.1:7777".parse().unwrap();
    let client_addr: SocketAddr = "127.0.0.1:8888".parse().unwrap();
    let mut client: Peer<Client> = Peer::<Client>::new(&client_addr, server_addr);

    let _ = client.connect(); // .await
    let world_state = vec![0x0; 32];

    'running: loop {
        // TODO(alex) 2021-01-27: This keeps the client running by:
        // 1. replying to hearbeat packets to the server;
        // 2. putting data packets received in the received data storage;
        // TODO(alex) 2021-01-27: Right now we accept every connection, but in the future we want
        // a ban list, so accepting connections will be part of the protocol API.
        // For the client, this means that we have to check if the connection was accepted, denied,
        // and get a cause. So the connection packet must have a cause in its body, giving strength
        // to the different packet structs idea.
        // let new_connections? = server.connection_requests();
        // server.deny_connection(new_connections[1]);
        client.tick();

        let data = vec![0x1; 32];
        // TODO(alex) 2021-01-28: Is this the exact same as the server? Do we need multiple ids?
        // The answer I have right now is, probably yes, if we have host migration then the client
        // needs to know that these changes are coming from this new server.
        let received_world_changes = client.retrieve(); // .await ??
        for (id, world_change) in received_world_changes {
            // TODO(alex) 2021-01-28: This is different from a simple `update` function, as this
            // exists to prevent networked update problems (packets out of order making the user
            // move to a wrong position, then come back, and move again).
            // This also highlights a flaw in the protocol, it has no mechanism to tell the user
            // which message is the freshest. The protocol will keep a `Timer` coupled with each
            // `Packet`, but should I put this in the API and let the user check this data, or
            // should the user insert a sequence into the `Body` of a packet and keep track of this
            // themselves? Be careful that this is not just about having a timer, as we could end up
            // getting a fresh packet with `05:30` but `sequence: 120`, and another with time
            // `05:31` and `sequence: 119`, making the 'newest' (by time) packet not being the
            // actual latest packet.
            // It ties in with the foundational aspects of the protocol, as in, do resends update
            // packet `sequence`, or does resending means copy + send again?

            // world_state.sync(world_change);
        }

        // TODO(alex) 2021-01-27: Send actually means `enqueue`, as I don't think immediately
        // sending is viable (or desirable).
        // Do we even have to `await` if this actually means to enqueue?
        client.enqueue(data); // .await ??
    }
}

fn server_main() {
    let server_addr = "127.0.0.1:7777".parse().unwrap();
    let client_addr = "127.0.0.1:8888";

    let mut server: Peer<Server> = Peer::<Server>::new(&server_addr);
    let world_state = vec![0x0; 32];

    'running: loop {
        // TODO(alex) 2021-01-27: This keeps the server running by:
        // 1. sending hearbeat packets to clients;
        // 2. putting data packets received in the received data storage;
        // TODO(alex) 2021-01-27: Right now we accept every connection, but in the future we want
        // a ban list, so accepting connections will be part of the protocol API.
        // let new_connections? = server.connection_requests();
        // server.deny_connection(new_connections[1]);
        server.tick();

        let new_world_state = vec![0x1; 32];
        // TODO(alex) 2021-01-27: Send actually means `enqueue`, as I don't think immediately
        // sending is viable (or desirable).
        // Do we even have to `await` if this actually means to enqueue?
        server.enqueue(new_world_state); // .await ??

        // TODO(alex) 2021-01-28: Receive retrieves all messages that the underlying protocol has
        // stored, but it also needs to move these messages into some form of `Retrieved(Packet)`
        // state, otherwise calling it again would keep returning the same packets.
        let received_world_changes = server.retrieve(); // .await ??
        for (_, world_change) in received_world_changes {
            // world_state.update(world_change);
        }
        // let (player_1_id, player_1_changes) =
        //     received_world_changes.find(|(id, world_changes)| world_changes.player_id == 1)?;
        // if player_1_changes.health < 0 {
        // TODO(alex) 2021-01-28: Not thinking about bans right now, but this could probably
        // just mark this host as banned, and drop the connection. The `Disconnected` ->
        // `ConnectionRequest` handling should have a special case to check if the host is in
        // a ban list, and ignore the connection request, or whatever.
        let player_1 = 1;
        server.ban_host(player_1);
        // }
        // let world_state_changes = world_state.delta().serialize();
        // TODO(alex) 2021-01-27: Work priority in the future, right now I would rather not
        // deal with handling this.
        // server.send_with_priority(Priority::High, id, dead_player_world_state);
        let world_state_changes = vec![0x2; 32];
        server.enqueue(world_state_changes); // .await
    }
}

fn main() {}
