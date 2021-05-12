use std::time::Duration;

use antarc::net::NetManager;
use log::debug;

fn main() {
    env_logger::init();
    client_main();
}

fn client_main() {
    let server_addr = "127.0.0.1:7777".parse().unwrap();
    let client_addr = "127.0.0.1:8888".parse().unwrap();
    let mut client = NetManager::new_client(&client_addr);
    client.connect(&server_addr);

    // .await ?
    // net_client.connect(&server_addr);
    // TODO(alex): 2021-02-20: This is part of the network manager side of things. When getting a
    // `Peer<Client<Connecting>>`, it keeps ticking until the client is actually connected to the
    // server. The user API won't be calling these inner `tick` functions, it'll call a
    // `NetworkManager.tick`.
    // let mut net_client = client_connecting.connected();
    // let world_state = vec![0x0; 32];

    loop {
        client.tick();
        std::thread::sleep(Duration::from_millis(150));
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
        // net_client.poll();

        // let data = vec![0x1; 32];
        // TODO(alex) 2021-01-28: Is this the exact same as the server? Do we need multiple ids?
        // The answer I have right now is, probably yes, if we have host migration then the client
        // needs to know that these changes are coming from this new server.
        // TODO(alex) 2021-02-17: Geting an error here because of what happens in the `Client`
        // creation. I don't want to have this as the API for creating a peer and managing the
        // network, the user should create some `NetworkManager`, set it's mode as either `Client`
        // or `Server` and use some `connect` that handles internal details for them, such as
        // connection retry and so on., but as it stands, the API requires the user to keep
        // creating a new binding everytime the `Peer` changes state, `let client = ...` is
        // leaking into user code.
        //
        // What I'm doing here is basically what the `NetworkManager` will have to do.
        // .await ??
        // let received_world_changes = net_client.receive();
        // for (id, world_change) in received_world_changes {
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
        // }

        // TODO(alex) 2021-01-27: Send actually means `enqueue`, as I don't think immediately
        // sending is viable (or desirable).
        // Do we even have to `await` if this actually means to enqueue?
        // net_client.send(data); // .await ??
    }
}
