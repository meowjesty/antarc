use std::{env, time::Duration};

// TODO(alex) [high] 2021-09-20: Time to tackle the network part, start simple, don't care
// about blocking, let's sketch out something usable, so that I can finally macro the heck out
// of this protocol (and reduce code duplication tenfold).
use antarc::{
    AntarcNet, Client, ClientEvent, ConnectionId, ProtocolEvent, ReliabilityType, SendTo, Server,
    ServerEvent,
};
use log::*;

fn main() {
    env::set_var("RUST_LOG", "debug");
    env_logger::init();
    run();
}

#[allow(unused)]
fn schedule_client<const VALUE: u8, const LEN: usize>(
    manager: &mut AntarcNet<Client>,
    reliability: ReliabilityType,
) {
    let scheduled = manager.schedule(reliability, SendTo::Broadcast, vec![VALUE; LEN]);
    info!("Client -> result of schedule call {:#?}", scheduled);
}

#[allow(unused)]
fn schedule_data_transfer_single(manager: &mut AntarcNet<Server>, connection_id: ConnectionId) {
    let scheduled = manager.schedule(
        ReliabilityType::Unreliable,
        SendTo::Single { connection_id },
        vec![0x1; 2],
    );
    info!("Server -> result of schedule call {:#?}", scheduled);
}

#[allow(unused)]
fn schedule_server<const VALUE: u8, const LEN: usize>(
    manager: &mut AntarcNet<Server>,
    reliability: ReliabilityType,
) {
    let scheduled = manager.schedule(reliability, SendTo::Broadcast, vec![VALUE; LEN]);
    info!("Server -> result of schedule call {:#?}", scheduled);
}

fn run() {
    let server_addr = "127.0.0.1:7777".parse().unwrap();
    let mut server = AntarcNet::new_server(server_addr, 32, Duration::from_secs(2));

    let client_addr = "127.0.0.1:8888".parse().unwrap();
    let mut client = AntarcNet::new_client(client_addr);
    client.connect(server_addr).unwrap();

    #[allow(unused)]
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

                            schedule_client::<0x3, 2>(&mut client, ReliabilityType::Unreliable);
                            // client.heartbeat(ReliabilityType::Unreliable);
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

                        // schedule_client::<0x4, 2>(&mut client, ReliabilityType::Unreliable);
                    }
                }
            }

            schedule_client::<0x5, 2>(&mut client, ReliabilityType::Unreliable);
            // schedule_client::<0x7, 1600>(&mut client, ReliabilityType::Unreliable);
            // schedule_client::<0x8, 1600>(&mut client, ReliabilityType::Reliable);
            // client.heartbeat(ReliabilityType::Unreliable);
            // client.heartbeat(ReliabilityType::Reliable);

            std::thread::sleep(Duration::from_millis(500));
        });

    #[allow(unused)]
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

                        // schedule_data_transfer_single(&mut server, connection_id);
                    }
                }
            }

            // server.heartbeat(ReliabilityType::Unreliable, SendTo::Broadcast);
            // server.heartbeat(ReliabilityType::Reliable, SendTo::Broadcast);
            schedule_server::<0x2, 2>(&mut server, ReliabilityType::Unreliable);
            // schedule_server::<0x6, 2>(&mut server, ReliabilityType::Reliable);
            // schedule_server::<0x9, 1600>(&mut server, ReliabilityType::Unreliable);
            // schedule_server::<0x10, 1600>(&mut server, ReliabilityType::Reliable);

            std::thread::sleep(Duration::from_millis(500));
        });

    loop {
        std::thread::yield_now();
        std::thread::sleep(Duration::from_millis(1500));
    }

    // client_thread.unwrap().join().unwrap();
    // server_thread.unwrap().join().unwrap();
}
