use std::collections::HashMap;

trait Connection {}

#[derive(Debug, PartialEq, Clone)]
struct Connecting;

#[derive(Debug, PartialEq, Clone)]
struct Awaiting;

#[derive(Debug, PartialEq, Clone)]
struct Connected;

#[derive(Debug, PartialEq, Clone)]
struct Host<State> {
    id: u64,
    state: State,
}

#[derive(Debug, PartialEq, Clone)]
struct Client {
    connecting: Option<Host<Connecting>>,
    awaiting: Option<Host<Awaiting>>,
    connected: Option<Host<Connected>>,
    // server: HashMap<impl Connection, Host>,
}

fn main() {
    let mut client = Client {
        connecting: None,
        awaiting: None,
        connected: None,
    };

    println!("Connect to server {:#?}", client);
    client.connecting = Some(Host {
        id: 1,
        state: Connecting,
    });
    println!("connecting... {:#?}", client);

    println!("Received connection ack");
    let connecting = client.connecting.take().unwrap();
    client.awaiting = Some(Host {
        id: connecting.id,
        state: Awaiting,
    });
    println!("awaiting connection accepted ... {:#?}", client);

    println!("Received connection accepted!");
    let awaiting = client.awaiting.take().unwrap();
    client.connected = Some(Host {
        id: awaiting.id,
        state: Connected,
    });
    println!("connected {:#?}", client);
}
