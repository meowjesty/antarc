use std::time::Duration;

use antarc::net::NetManager;
use log::{debug, error};

fn main() {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();
    client_main();
}

fn client_main() {
    debug!("Starting client application...");
    let server_addr = "127.0.0.1:7777".parse().unwrap();
    let client_addr = "127.0.0.1:8888".parse().unwrap();
    let mut client = NetManager::new_client(&client_addr);
    debug!("Created {:#?}.", client);

    debug!("Attempting connection to {:#?}", server_addr);
    // TODO(alex) 2021-05-18: Possible API if `connect` is async-like.
    // let connection_id = client.connect(&server_addr).unwrap();
    // debug!("Connected {:#?}.", connection_id);
    client.connect(&server_addr);

    let mut d_counter: i32 = 0;
    loop {
        if d_counter % 8 == 0 {
            // TODO(alex) 2021-05-18: Should we allow packets to be enqueued before the connection
            // is properly estabilished?
            let packet_id = client.enqueue((&d_counter.to_be_bytes()).to_vec());
            debug!("Enqueue test packet {:#?}", packet_id);

            if d_counter % 16 == 0 {
                debug!("Cancel test packet {:#?}", packet_id);
                let cancelled = client.cancel_packet(packet_id);
                debug!("Test packet cancellation result is {:#?}", cancelled);
            }
        }

        if d_counter % 6 == 0 {
            let retrieved_data = client.retrieve();
            for (connection_id, data) in retrieved_data.iter() {
                debug!("Retrieved {:#?} from {:#?}.", data, connection_id);
            }
        }

        match client.tick() {
            Ok(received_new_data) => {
                if received_new_data > 0 {
                    let retrieved_data = client.retrieve();
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
