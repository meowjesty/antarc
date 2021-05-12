use std::time::Duration;

use antarc::net::NetManager;
use log::debug;

fn main() {
    env_logger::init();
    server_main();
}

fn server_main() {
    let server_addr = "127.0.0.1:7777".parse().unwrap();
    // let client_addr = "127.0.0.1:8888";
    let mut server = NetManager::new_server(&server_addr);

    // let mut server: NetManager<Server> = NetManager::<Server>::new_server(&server_addr);
    // let world_state = vec![0x0; 32];

    loop {
        server.tick();

        std::thread::sleep(Duration::from_millis(150));
        // TODO(alex) 2021-01-27: This keeps the server running by:
        // 1. sending hearbeat packets to clients;
        // 2. putting data packets received in the received data storage;
        // TODO(alex) 2021-01-27: Right now we accept every connection, but in the future we want
        // a ban list, so accepting connections will be part of the protocol API.
        // let new_connections? = server.connection_requests();
        // server.deny_connection(new_connections[1]);
        // server.tick();

        // let new_world_state = vec![0x1; 32];
        // TODO(alex) 2021-01-27: Send actually means `enqueue`, as I don't think immediately
        // sending is viable (or desirable).
        // Do we even have to `await` if this actually means to enqueue?
        // server.enqueue(new_world_state); // .await ??

        // TODO(alex) 2021-01-28: Receive retrieves all messages that the underlying protocol has
        // stored, but it also needs to move these messages into some form of `Retrieved(Packet)`
        // state, otherwise calling it again would keep returning the same packets.
        // let received_world_changes = server.retrieve(); // .await ??
        // for (_, world_change) in received_world_changes {
        // world_state.update(world_change);
        // }
        // let (player_1_id, player_1_changes) =
        //     received_world_changes.find(|(id, world_changes)| world_changes.player_id == 1)?;
        // if player_1_changes.health < 0 {
        // TODO(alex) 2021-01-28: Not thinking about bans right now, but this could probably
        // just mark this host as banned, and drop the connection. The `Disconnected` ->
        // `ConnectionRequest` handling should have a special case to check if the host is in
        // a ban list, and ignore the connection request, or whatever.
        // let player_1 = 1;
        // server.ban_host(player_1);
        // }
        // let world_state_changes = world_state.delta().serialize();
        // TODO(alex) 2021-01-27: Work priority in the future, right now I would rather not
        // deal with handling this.
        // server.send_with_priority(Priority::High, id, dead_player_world_state);
        // let world_state_changes = vec![0x2; 32];
        // server.enqueue(world_state_changes); // .await
    }
}
