use std::{
    any::{Any, TypeId},
    collections::HashMap,
    fmt,
};

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

#[derive(Debug)]
struct Client {
    connecting: Vec<Host<Connecting>>,
    awaiting: Vec<Host<Awaiting>>,
    connected: Vec<Host<Connected>>,
}

fn main() {
    let mut client = Client {
        connecting: Vec::with_capacity(1),
        awaiting: Vec::with_capacity(1),
        connected: Vec::with_capacity(1),
    };

    println!("Connect to server {:#?}", client);
    client.connecting.insert(
        0,
        Host {
            id: 1,
            state: Connecting,
        },
    );
    println!("connecting... {:#?}", client);

    println!("Received connection ack");
    let connecting = client.connecting.remove(0);
    client.awaiting.insert(
        0,
        Host {
            id: connecting.id,
            state: Awaiting,
        },
    );
    println!("awaiting connection accepted ... {:#?}", client);

    println!("Received connection accepted!");
    let awaiting = client.awaiting.remove(0);
    client.connected.insert(
        0,
        Host {
            id: awaiting.id,
            state: Connected,
        },
    );
    println!("connected {:#?}", client);

    let insert_again = client.connected.remove(0);
    client.awaiting.insert(
        0,
        Host {
            id: insert_again.id,
            state: Awaiting,
        },
    );
    println!("connected {:#?}", client);
}
