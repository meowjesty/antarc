use std::time::Duration;

use antarc::net::NetManager;
use log::debug;

fn main() {
    env_logger::init();
    server_main();
}

// TODO(alex) 2021-05-18: This has to be sole focus, look at the `client/main` and try to derive a
// similar API for the server with a few things in mind:
//
// 1. the server `enqueue` probably takes a `ConnectionId` indicating to whom the packet will be
// sent;
fn server_main() {
    let server_addr = "127.0.0.1:7777".parse().unwrap();
    // let client_addr = "127.0.0.1:8888";
    let mut server = NetManager::new_server(&server_addr);

    // let mut server: NetManager<Server> = NetManager::<Server>::new_server(&server_addr);
    // let world_state = vec![0x0; 32];

    loop {
        server.tick();

        std::thread::sleep(Duration::from_millis(150));
    }
}
