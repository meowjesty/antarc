use std::time::Duration;

use antarc_dummy::{AntarcEvent, DummyManager};
use log::{debug, error, info, warn};

fn main() {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();
    client_main();
}

fn client_main() {
    let server_addr = "127.0.0.1:7777".parse().unwrap();
    let client_addr = "127.0.0.1:8888".parse().unwrap();
    let mut client = DummyManager::new_client(client_addr);

    client.connect(server_addr);

    loop {
        // TODO(alex) [low] 2021-08-01: To avoid allocating events over and over, the user may pass
        // an events vector or register some global event list:
        // let events = Vec::with_capacity(1024);
        // client.register(events);
        //
        // ADD(alex) [low] 2021-08-01: I've tried returning the `DrainIter` from `poll`, but it
        // ends up borrowing `client` twice, here and in `schedule`.
        let mut events = client.poll().collect::<Vec<_>>();
        for event in events.drain(..) {
            match event {
                AntarcEvent::Fail(fail) => error!("{:#?}", fail),
                AntarcEvent::ConnectionRequest {
                    connection_id,
                    remote,
                } => {
                    // TODO(alex) [low] 2021-08-01: How do I make this impossible event disappear?
                    // I would need to separate `ClientEvent` and `ServerEvent`, so the
                    // `EventSystem` will be different for `Antarc<Client>` and `Antarc<Server>`.
                    warn!("Client doesn't handle connection request.");
                    continue;
                }
                AntarcEvent::ConnectionAccepted { connection_id } => {
                    info!("Received connection accepted from {:#?}.", connection_id);
                    client.schedule(false, vec![0x3; 30])
                }
                AntarcEvent::DataTransfer {
                    connection_id,
                    payload,
                } => {
                    info!("Received {:#?} from {:#?}.", payload.len(), connection_id);
                    client.schedule(false, vec![0x4; 40]);
                }
            }
        }

        client.schedule(false, vec![0x5; 50]);

        std::thread::sleep(Duration::from_millis(500));
    }
}
