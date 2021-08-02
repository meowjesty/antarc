use std::{env, time::Duration};

use antarc_dummy::{AntarcEvent, DummyManager, SendTo};
use log::*;

fn main() {
    env::set_var("RUST_LOG", "debug");
    env_logger::init();
    server_main();
}

fn server_main() {
    let server_addr = "127.0.0.1:7777".parse().unwrap();
    let mut server = DummyManager::new_server(server_addr);

    loop {
        // TODO(alex) [low] 2021-08-01: To avoid allocating events over and over, the user may pass
        // an events vector or register some global event list:
        // let events = Vec::with_capacity(1024);
        // server.register(events);
        //
        // ADD(alex) [low] 2021-08-01: I've tried returning the `DrainIter` from `poll`, but it
        // ends up borrowing `server` twice, here and in `schedule`.
        let mut events = server.poll().collect::<Vec<_>>();
        for event in events.drain(..) {
            match event {
                AntarcEvent::Fail(fail) => error!("{:#?}", fail),
                AntarcEvent::ConnectionRequest {
                    connection_id,
                    remote,
                } => {
                    info!(
                        "Received a connection request from {:#?} with id {:#?}",
                        remote, connection_id
                    );

                    server.accept_connection(connection_id);
                }
                AntarcEvent::ConnectionAccepted { .. } => {
                    // TODO(alex) [low] 2021-08-01: How do I make this impossible event disappear?
                    // I would need to separate `ClientEvent` and `ServerEvent`, so the
                    // `EventSystem` will be different for `Antarc<Client>` and `Antarc<Server>`.
                    warn!("Server doesn't handle connection accepted.");
                    continue;
                }
                AntarcEvent::DataTransfer {
                    connection_id,
                    payload,
                } => {
                    info!("Received {:#?} from {:#?}.", payload.len(), connection_id);

                    server.schedule(false, SendTo::Single { connection_id }, vec![0x1; 10]);
                }
            }
        }

        server.schedule(false, SendTo::Broadcast, vec![0x2; 20]);

        std::thread::sleep(Duration::from_millis(500));
    }
}
