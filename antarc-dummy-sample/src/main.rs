use std::{env, time::Duration};

use antarc_dummy::{
    ClientEvent, DummyManager, ProtocolEvent, ReliabilityType, SendTo, ServerEvent,
};
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

    let (client_tx, client_rx) = std::sync::mpsc::channel::<Vec<Vec<u8>>>();
    let (server_tx, server_rx) = std::sync::mpsc::channel::<Vec<Vec<u8>>>();

    let client_thread = std::thread::Builder::new()
        .name("Client thread".to_string())
        .spawn(move || loop {
            // Client:
            for event in client.poll().collect::<Vec<_>>().drain(..) {
                match event {
                    ProtocolEvent::Fail(fail) => error!("{:#?}", fail),
                    ProtocolEvent::ServiceEvent(service_event) => match service_event {
                        ClientEvent::ConnectionAccepted { connection_id } => {
                            info!(
                                "Client -> received connection accepted from {:#?}.",
                                connection_id
                            );

                            let scheduled =
                                client.schedule(ReliabilityType::Unreliable, vec![0x3; 2]);
                            info!("Client -> result of schedule call {:#?}", scheduled);
                        }
                    },
                    ProtocolEvent::DataTransfer {
                        connection_id,
                        payload,
                    } => {
                        info!(
                            "Client -> received {:#?} from {:#?}.",
                            payload.len(),
                            connection_id
                        );

                        let scheduled = client.schedule(ReliabilityType::Unreliable, vec![0x4; 2]);
                        info!("Client -> result of schedule call {:#?}", scheduled);
                    }
                }
            }

            let scheduled = client.schedule(ReliabilityType::Unreliable, vec![0x5; 2]);
            info!("Client -> result of schedule call {:#?}", scheduled);

            let scheduled = client.schedule(ReliabilityType::Unreliable, vec![0x7; 1600]);
            info!("Client -> result of schedule call {:#?}", scheduled);

            let scheduled = client.schedule(ReliabilityType::Reliable, vec![0x8; 1600]);
            info!("Client -> result of schedule call {:#?}", scheduled);

            let _ = client_tx.send(client.dummy_sender.clone());
            if let Ok(packets) = server_rx.try_recv() {
                client.dummy_receiver = packets;
            }

            client.dummy_sender.drain(..);
            std::thread::sleep(Duration::from_millis(500));
        });

    let server_thread = std::thread::Builder::new()
        .name("Server thread".to_string())
        .spawn(move || loop {
            // TODO(alex) [low] 2021-08-01: To avoid allocating events over and over, the user may
            // pass an events vector or register some global event list:
            // let events = Vec::with_capacity(1024);
            // server.register(events);
            //
            // ADD(alex) [low] 2021-08-01: I've tried returning the `DrainIter` from `poll`, but it
            // ends up borrowing `server` twice, here and in `schedule`.
            //
            // Server:
            for event in server.poll().collect::<Vec<_>>().drain(..) {
                match event {
                    ProtocolEvent::Fail(fail) => error!("{:#?}", fail),
                    ProtocolEvent::ServiceEvent(service_event) => match service_event {
                        ServerEvent::ConnectionRequest {
                            connection_id,
                            remote,
                        } => {
                            info!(
                                "Server -> received a connection request from {:#?} with id {:#?}",
                                remote, connection_id
                            );
                        }
                    },
                    ProtocolEvent::DataTransfer {
                        connection_id,
                        payload,
                    } => {
                        info!(
                            "Server -> received {:#?} from {:#?}.",
                            payload.len(),
                            connection_id
                        );

                        let scheduled = server.schedule(
                            ReliabilityType::Unreliable,
                            SendTo::Single { connection_id },
                            vec![0x1; 2],
                        );
                        info!("Server -> result of schedule call {:#?}", scheduled);
                    }
                }
            }

            let scheduled =
                server.schedule(ReliabilityType::Unreliable, SendTo::Broadcast, vec![0x2; 2]);
            info!("Server -> result of schedule call {:#?}", scheduled);

            let scheduled =
                server.schedule(ReliabilityType::Reliable, SendTo::Broadcast, vec![0x6; 2]);
            info!("Server -> result of schedule call {:#?}", scheduled);

            let scheduled = server.schedule(
                ReliabilityType::Unreliable,
                SendTo::Broadcast,
                vec![0x9; 1600],
            );
            info!("Server -> result of schedule call {:#?}", scheduled);

            let scheduled = server.schedule(
                ReliabilityType::Reliable,
                SendTo::Broadcast,
                vec![0x10; 1600],
            );
            info!("Server -> result of schedule call {:#?}", scheduled);

            let _ = server_tx.send(server.dummy_sender.clone());
            if let Ok(packets) = client_rx.try_recv() {
                server.dummy_receiver = packets;
            }

            server.dummy_sender.drain(..);
            std::thread::sleep(Duration::from_millis(500));
        });

    loop {
        std::thread::sleep(Duration::from_millis(1500));
    }

    client_thread.unwrap().join().unwrap();
    server_thread.unwrap().join().unwrap();
}
