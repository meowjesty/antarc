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

    let client_addr = "127.0.0.1:8888".parse().unwrap();
    let mut client = DummyManager::new_client(client_addr);
    client.connect(server_addr).unwrap();

    loop {
        // TODO(alex) [low] 2021-08-01: To avoid allocating events over and over, the user may pass
        // an events vector or register some global event list:
        // let events = Vec::with_capacity(1024);
        // server.register(events);
        //
        // ADD(alex) [low] 2021-08-01: I've tried returning the `DrainIter` from `poll`, but it
        // ends up borrowing `server` twice, here and in `schedule`.

        // Server:
        for event in server.poll().collect::<Vec<_>>().drain(..) {
            match event {
                AntarcEvent::Fail(fail) => error!("{:#?}", fail),
                AntarcEvent::ConnectionRequest {
                    connection_id,
                    remote,
                } => {
                    info!(
                        "Server -> received a connection request from {:#?} with id {:#?}",
                        remote, connection_id
                    );

                    server.accept_connection(connection_id);
                }
                AntarcEvent::ConnectionAccepted { .. } => {
                    // TODO(alex) [low] 2021-08-01: How do I make this impossible event disappear?
                    // I would need to separate `ClientEvent` and `ServerEvent`, so the
                    // `EventSystem` will be different for `Antarc<Client>` and `Antarc<Server>`.
                    warn!("Server -> cannot handle connection accepted.");
                    // FIXME(alex) [vhigh] 2021-08-05: Server is decoding a packet that was supposed
                    // to be a data transfer as a connection accepted.
                    continue;
                }
                AntarcEvent::DataTransfer {
                    connection_id,
                    payload,
                } => {
                    info!(
                        "Server -> received {:#?} from {:#?}.",
                        payload.len(),
                        connection_id
                    );

                    server.schedule(false, SendTo::Single { connection_id }, vec![0x1; 2]);
                }
            }
        }

        // Client:
        for event in client.poll().collect::<Vec<_>>().drain(..) {
            match event {
                AntarcEvent::Fail(fail) => error!("{:#?}", fail),
                AntarcEvent::ConnectionRequest { .. } => {
                    // TODO(alex) [low] 2021-08-01: How do I make this impossible event disappear?
                    // I would need to separate `ClientEvent` and `ServerEvent`, so the
                    // `EventSystem` will be different for `Antarc<Client>` and `Antarc<Server>`.
                    warn!("Client -> cannot handle connection request.");
                    continue;
                }
                AntarcEvent::ConnectionAccepted { connection_id } => {
                    info!(
                        "Client -> received connection accepted from {:#?}.",
                        connection_id
                    );
                    client.schedule(false, vec![0x3; 2])
                }
                AntarcEvent::DataTransfer {
                    connection_id,
                    payload,
                } => {
                    info!(
                        "Client -> received {:#?} from {:#?}.",
                        payload.len(),
                        connection_id
                    );
                    client.schedule(false, vec![0x4; 2]);
                }
            }
        }

        client.schedule(false, vec![0x5; 2]);
        server.schedule(false, SendTo::Broadcast, vec![0x2; 2]);

        server.dummy_receiver = client.dummy_sender.clone();
        client.dummy_receiver = server.dummy_sender.clone();

        client.dummy_sender.drain(..);
        server.dummy_sender.drain(..);

        std::thread::sleep(Duration::from_millis(500));
    }
}
