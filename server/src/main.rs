use std::{env, time::Duration};

use antarc::net::NetManager;
use log::{debug, error};

fn main() {
    env::set_var("RUST_LOG", "debug");
    env_logger::init();
    server_main();
}

// TODO(alex) 2021-05-18: This has to be sole focus, look at the `server/main` and try to derive a
// similar API for the server with a few things in mind:
//
// 1. the server `enqueue` probably takes a `ConnectionId` indicating to whom the packet will be
// sent;
fn server_main() {
    let server_addr = "127.0.0.1:7777".parse().unwrap();
    // let server_addr = "127.0.0.1:8888";
    let mut server = NetManager::new_server(&server_addr);

    // let mut server: NetManager<Server> = NetManager::<Server>::new_server(&server_addr);
    // let world_state = vec![0x0; 32];

    let mut d_counter: i32 = 0;
    loop {
        if d_counter % 8 == 0 {
            // TODO(alex) 2021-05-18: Should we allow packets to be enqueued before the connection
            // is properly estabilished?
            let packet_id = server.enqueue((&d_counter.to_be_bytes()).to_vec());
            debug!("Enqueue test packet {:#?}", packet_id);

            if d_counter % 16 == 0 {
                debug!("Cancel test packet {:#?}", packet_id);
                let cancelled = server.cancel_packet(packet_id);
                debug!("Test packet cancellation result is {:#?}", cancelled);
            }
        }

        if d_counter % 6 == 0 {
            let retrieved_data = server.retrieve();
            for (connection_id, data) in retrieved_data.iter() {
                debug!("Retrieved {:#?} from {:#?}.", data, connection_id);
            }
        }

        match server.tick() {
            Ok(received_new_data) => {
                if received_new_data > 0 {
                    let retrieved_data = server.retrieve();
                    for (connection_id, data) in retrieved_data.iter() {
                        debug!("Retrieved {:#?} from {:#?}.", data, connection_id);
                    }
                }
            }
            Err(fail) => {
                error!("Tick returned some error {:#?}.", fail);
            }
        }

        d_counter += 1;
        std::thread::sleep(Duration::from_millis(1500));
    }
}
