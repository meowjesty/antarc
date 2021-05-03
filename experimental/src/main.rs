#![allow(dead_code, unused_variables, unused_imports)]
#![feature(write_all_vectored)]

use std::{
    convert::TryInto,
    io::{Cursor, IoSlice, Write},
    mem,
    net::SocketAddr,
    num::{NonZeroU16, NonZeroU32},
    time::Duration,
};

use antarc::{net::NetManager, server::Server};
use hecs::{Entity, With, Without, World};
use log::{debug, error, info, trace, warn};
use mem::size_of;

fn main() {
    env_logger::init();
    let client = std::thread::Builder::new()
        .name("Client".to_string())
        .spawn(|| client_main())
        .unwrap();
    let server = std::thread::Builder::new()
        .name("Server".to_string())
        .spawn(|| server_main())
        .unwrap();

    let _ = client.join();
    let _ = server.join();
}

fn client_main() {
    let server_addr: SocketAddr = "127.0.0.1:7777".parse().unwrap();
    let client_addr: SocketAddr = "127.0.0.1:8888".parse().unwrap();
    let mut net_client = NetManager::new_client(&client_addr);
    net_client.connect(&server_addr);

    // .await ?
    // net_client.connect(&server_addr);
    // TODO(alex): 2021-02-20: This is part of the network manager side of things. When getting a
    // `Peer<Client<Connecting>>`, it keeps ticking until the client is actually connected to the
    // server. The user API won't be calling these inner `tick` functions, it'll call a
    // `NetworkManager.tick`.
    // let mut net_client = client_connecting.connected();
    // let world_state = vec![0x0; 32];

    loop {
        net_client.tick();
        std::thread::sleep(Duration::from_millis(150));
        // TODO(alex) 2021-01-27: This keeps the client running by:
        // 1. replying to hearbeat packets to the server;
        // 2. putting data packets received in the received data storage;
        // TODO(alex) 2021-01-27: Right now we accept every connection, but in the future we want
        // a ban list, so accepting connections will be part of the protocol API.
        // For the client, this means that we have to check if the connection was accepted, denied,
        // and get a cause. So the connection packet must have a cause in its body, giving strength
        // to the different packet structs idea.
        // let new_connections? = server.connection_requests();
        // server.deny_connection(new_connections[1]);
        // net_client.poll();

        // let data = vec![0x1; 32];
        // TODO(alex) 2021-01-28: Is this the exact same as the server? Do we need multiple ids?
        // The answer I have right now is, probably yes, if we have host migration then the client
        // needs to know that these changes are coming from this new server.
        // TODO(alex) 2021-02-17: Geting an error here because of what happens in the `Client`
        // creation. I don't want to have this as the API for creating a peer and managing the
        // network, the user should create some `NetworkManager`, set it's mode as either `Client`
        // or `Server` and use some `connect` that handles internal details for them, such as
        // connection retry and so on., but as it stands, the API requires the user to keep
        // creating a new binding everytime the `Peer` changes state, `let client = ...` is
        // leaking into user code.
        //
        // What I'm doing here is basically what the `NetworkManager` will have to do.
        // .await ??
        // let received_world_changes = net_client.receive();
        // for (id, world_change) in received_world_changes {
        // TODO(alex) 2021-01-28: This is different from a simple `update` function, as this
        // exists to prevent networked update problems (packets out of order making the user
        // move to a wrong position, then come back, and move again).
        // This also highlights a flaw in the protocol, it has no mechanism to tell the user
        // which message is the freshest. The protocol will keep a `Timer` coupled with each
        // `Packet`, but should I put this in the API and let the user check this data, or
        // should the user insert a sequence into the `Body` of a packet and keep track of this
        // themselves? Be careful that this is not just about having a timer, as we could end up
        // getting a fresh packet with `05:30` but `sequence: 120`, and another with time
        // `05:31` and `sequence: 119`, making the 'newest' (by time) packet not being the
        // actual latest packet.
        // It ties in with the foundational aspects of the protocol, as in, do resends update
        // packet `sequence`, or does resending means copy + send again?

        // world_state.sync(world_change);
        // }

        // TODO(alex) 2021-01-27: Send actually means `enqueue`, as I don't think immediately
        // sending is viable (or desirable).
        // Do we even have to `await` if this actually means to enqueue?
        // net_client.send(data); // .await ??
    }
}

fn server_main() {
    let server_addr = "127.0.0.1:7777".parse().unwrap();
    // let client_addr = "127.0.0.1:8888";
    let mut server = NetManager::<Server>::new_server(&server_addr);

    // let mut server: NetManager<Server> = NetManager::<Server>::new_server(&server_addr);
    // let world_state = vec![0x0; 32];

    loop {
        server.tick();
        std::thread::sleep(Duration::from_millis(150));
        // TODO(alex) 2021-01-27: This keeps the server running by:
        // 1. sending hearbeat packets to clients;
        // 2. putting data packets received in the received data storage;
        // TODO(alex) 2021-01-27: Right now we accept every connection, but in the future we want
        // a ban list, so accepting connections will be part of the protocol API.
        // let new_connections? = server.connection_requests();
        // server.deny_connection(new_connections[1]);
        // server.tick();

        // let new_world_state = vec![0x1; 32];
        // TODO(alex) 2021-01-27: Send actually means `enqueue`, as I don't think immediately
        // sending is viable (or desirable).
        // Do we even have to `await` if this actually means to enqueue?
        // server.enqueue(new_world_state); // .await ??

        // TODO(alex) 2021-01-28: Receive retrieves all messages that the underlying protocol has
        // stored, but it also needs to move these messages into some form of `Retrieved(Packet)`
        // state, otherwise calling it again would keep returning the same packets.
        // let received_world_changes = server.retrieve(); // .await ??
        // for (_, world_change) in received_world_changes {
        // world_state.update(world_change);
        // }
        // let (player_1_id, player_1_changes) =
        //     received_world_changes.find(|(id, world_changes)| world_changes.player_id == 1)?;
        // if player_1_changes.health < 0 {
        // TODO(alex) 2021-01-28: Not thinking about bans right now, but this could probably
        // just mark this host as banned, and drop the connection. The `Disconnected` ->
        // `ConnectionRequest` handling should have a special case to check if the host is in
        // a ban list, and ignore the connection request, or whatever.
        // let player_1 = 1;
        // server.ban_host(player_1);
        // }
        // let world_state_changes = world_state.delta().serialize();
        // TODO(alex) 2021-01-27: Work priority in the future, right now I would rather not
        // deal with handling this.
        // server.send_with_priority(Priority::High, id, dead_player_world_state);
        // let world_state_changes = vec![0x2; 32];
        // server.enqueue(world_state_changes); // .await
    }
}

#[derive(Debug)]
struct Address {
    addr: u32,
}

#[derive(Debug)]
struct Packet {
    payload: u32,
    kind: String,
}

#[derive(Debug)]
struct Received;

#[derive(Debug)]
struct Sent;

#[derive(Debug)]
struct Host {
    tracker: u32,
}

#[derive(Debug)]
struct HostPacket {
    host_id: Entity,
    packet_id: Entity,
}

#[derive(Debug)]
struct Source {
    host_id: Entity,
}

#[derive(Debug)]
struct Destination {
    host_id: Entity,
}

#[derive(Debug)]
struct ConnectionRequest;

mod test_constants {
    pub const ZERO: u16 = 0x00;
    pub const FULL: u16 = 0xff;
}

fn test_stuff_main() {
    let mut world = World::new();

    let host = world.spawn((Host { tracker: 0 }, Address { addr: 1 }));

    for i in 0..2 {
        let received_packet = world.spawn((
            Packet {
                payload: 10 + i,
                kind: format!("Received {:?}", i),
            },
            Received,
            Address { addr: 1 },
        ));
    }

    for i in 0..2 {
        let sent_packet = world.spawn((
            Packet {
                payload: 20 + i,
                kind: format!("Sent {:?}", i),
            },
            Sent,
            Address { addr: 1 },
        ));
    }

    // Systems can be simple for loops
    let mut received_packets = Vec::new();
    println!("Received packets!");
    for (packet_id, (packet, address, received)) in world
        .query::<Without<(Host, Sent), (&Packet, &Address, &Received)>>()
        .iter()
    {
        println!("{:#?} {:#?} {:#?}", packet_id, packet, address);
        received_packets.push(packet_id);
    }

    while let Some(packet_id) = received_packets.pop() {
        println!("(Received) Adding {:?} to {:?}", host, packet_id);
        let _ = world
            .insert(packet_id, (Source { host_id: host },))
            .unwrap();
    }

    let mut sent_packets = Vec::new();
    println!("Sent packets!");
    for (packet_id, (packet, address, sent)) in world
        .query::<Without<(Host, Received), (&Packet, &Address, &Sent)>>()
        .iter()
    {
        println!("{:#?} {:#?} {:#?}", packet_id, packet, address);
        sent_packets.push(packet_id);
    }

    while let Some(packet_id) = sent_packets.pop() {
        println!("(Sent) Adding {:?} to {:?}", host, packet_id);
        let _ = world
            .insert(packet_id, (Destination { host_id: host },))
            .unwrap();
    }

    let sourceless_packet = world.spawn((
        Packet {
            payload: 10 + 5,
            kind: format!("Received {:?} (no source)", 5),
        },
        Received,
        Address { addr: 25 },
        ConnectionRequest,
    ));

    let destinationless_packet = world.spawn((
        Packet {
            payload: 20 + 5,
            kind: format!("Sent {:?} (no destination)", 5),
        },
        Sent,
        Address { addr: 5 },
    ));

    println!("Hosts");
    for (host_id, host) in world.query::<With<Address, &Host>>().iter() {
        println!("{:#?} {:#?} ", host_id, host);
    }

    println!("Packets");
    for (packet_id, packet) in world.query::<With<Address, &Packet>>().iter() {
        println!("{:#?} {:#?} ", packet_id, packet);
    }

    println!("Packets with destination");
    for (packet_id, destination) in world.query::<(&Destination,)>().iter() {
        println!("Destination {:#?} {:#?}", packet_id, destination);
    }

    println!("Packets with source");
    for (packet_id, source) in world.query::<(&Source,)>().iter() {
        println!("Source {:#?} {:#?}", packet_id, source);
    }

    // TODO(alex) 2021-03-27: Adding `Sent` or `Received` (proper type attribution for the
    // `HosPacket` table) allows us to filter for it during many-to-many query.
    println!("Host <-> Packets (only sent, with destination)");
    for (packet_id, (packet, sent, destination, address)) in world
        .query::<(&Packet, &Sent, &Destination, &Address)>()
        .iter()
    {
        println!("{:#?} {:#?} {:#?}", packet_id, packet, address);
        let mut host_query = world.query_one::<&Host>(destination.host_id).unwrap();
        let host = host_query.get();
        println!("{:#?}", host);
    }

    println!("Host <-> Packets (only received, with source)");
    for (packet_id, (packet, received, source, address)) in world
        .query::<(&Packet, &Received, &Source, &Address)>()
        .iter()
    {
        println!(
            "{:#?} {:#?} {:#?} {:#?}",
            packet_id, packet, source, address
        );
        let mut host_query = world.query_one::<&Host>(source.host_id).unwrap();
        let host = host_query.get();
        println!("{:#?}", host);
    }

    println!("Host <-> Packets (received, unknown hosts)");
    for (packet_id, (packet,)) in world
        .query::<(&Packet,)>()
        .with::<Received>()
        .without::<Source>()
        .iter()
    {
        println!("{:#?} {:#?} ", packet_id, packet);
    }

    println!("Host <-> Packets (sent, unknown hosts)");
    for (packet_id, (packet, received)) in world
        .query::<(&Packet, &Sent)>()
        .without::<Destination>()
        .iter()
    {
        println!("{:#?} {:#?} ", packet_id, packet);
    }

    println!("Host <-> Packets (received, unknown hosts, connection request)");
    for (packet_id, (packet,)) in world
        .query::<(&Packet,)>()
        .with::<Received>()
        .with::<ConnectionRequest>()
        .without::<Source>()
        .iter()
    {
        println!("{:#?} {:#?} ", packet_id, packet);
    }

    println!("Host <-> Packets (sourceless + destinationless)");
    for (packet_id, (packet,)) in world
        .query::<(&Packet,)>()
        .without::<Source>()
        .without::<Destination>()
        .iter()
    {
        println!("{:#?} {:#?} ", packet_id, packet);
    }

    println!("Test out Cursor WRITE");
    {
        let first_bytes = u32::to_be_bytes(0x1);
        let second_bytes = u32::to_be_bytes(0x8);
        let third_bytes = u32::to_be_bytes(0xa);

        let mut first_buffers = [
            IoSlice::new(&first_bytes),
            IoSlice::new(&second_bytes),
            IoSlice::new(&third_bytes),
        ];

        let mut cursor = Cursor::new(Vec::with_capacity(128));
        let _ = cursor.write_all_vectored(&mut first_buffers).unwrap();
        println!("first position {:#?}", cursor.position());

        let fourth_bytes = u32::to_be_bytes(0x10);
        let _ = cursor.write_all(&fourth_bytes).unwrap();
        println!("second position {:#?}", cursor.position());
    }

    println!("Test out Cursor READ");
    {
        let buffer: Vec<u8> = vec![
            0x0, 0x1, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7, 0x8, 0x9, 0xa, 0xb, 0xc, 0xd, 0xe, 0xf,
        ];
        let crc32_position = buffer.len() - mem::size_of::<u32>();

        let crc32_bytes: &[u8; 4] = buffer[crc32_position..].try_into().unwrap();
        let crc32 = u32::from_be_bytes(*crc32_bytes);
        println!("crc32 is {:#?} \n\t {:#?}", crc32, crc32_bytes);

        let sentinel = vec![0xa, 0xa, 0xa, 0xa];
        let buffer_with_sentinel: Vec<u8> = [&sentinel, &buffer[..]].concat();
        println!("buffer with sentinel {:#?}", buffer_with_sentinel);

        let reading_into_vec = buffer_with_sentinel[0..4].to_vec();
        println!("read into vec {:#?}", reading_into_vec);
    }

    println!("Test out status code bitflag");
    {
        let status_code: u16 = 0b0010_0100_0011_0001;
        let lower_part: u16 = status_code & 0b1111;
        let high_part: u16 = (status_code >> 4) & 0b1111_1111_1111;
        println!(
            "status_code {:#?} lower_part {:#?} high_part {:#?}",
            status_code, lower_part, high_part
        );
        println!(
            "{:#018b}\n{:#018b}\n{:#018b}",
            status_code, lower_part, high_part
        );

        let status_code: u16 = 0b0010_0100_0011_0001;
        let lower_part: u16 = status_code & 0xf;
        let high_part: u16 = (status_code >> 4) & 0xfff;
        println!(
            "status_code {:#?} lower_part {:#?} high_part {:#?}",
            status_code, lower_part, high_part
        );
        println!(
            "{:#018b}\n{:#018b}\n{:#018b}",
            status_code, lower_part, high_part
        );
    }

    println!("Test out match on u16 + guard");
    {
        // const ZERO: u16 = 0x00;
        // const FULL: u16 = 0xff;
        let status_code: u16 = 0xff;
        // let optional: Option<u16> = Some(0xffff);
        let optional: Option<u16> = None;

        println!();
        println!("matching on constant!");
        match status_code {
            0x0 if optional.is_none() => {
                println!("entered 0x0 if optional.is_none()");
                println!(
                    "status_code {:#0x} with optional {:#?}",
                    status_code, optional
                );
            }
            0x0 if optional.is_some() => {
                println!("entered 0x0 if optional.is_some()");
                println!(
                    "status_code {:#0x} with optional {:#?}",
                    status_code, optional
                );
            }
            0xff if optional.is_none() => {
                println!("entered 0xff if optional.is_none()");
                println!(
                    "status_code {:#0x} with optional {:#?}",
                    status_code, optional
                );
            }
            0xff if optional.is_some() => {
                println!("entered 0xff if optional.is_some()");
                println!(
                    "status_code {:#0x} with optional {:#?}",
                    status_code, optional
                );
            }
            other => {
                println!("entered other");
                println!(
                    "status_code {:#0x} with optional {:#?} other {:#0x}",
                    status_code, optional, other
                );
            }
        }

        /*
        println!();
        println!("matching on if guard!");
        match status_code {
            x if optional.is_none() && x == ZERO => {
                println!("entered 0x0 if optional.is_none() ZERO");
                println!(
                    "status_code {:#0x} with optional {:#?} and x {:#0x}",
                    status_code, optional, x
                );
            }
            x if optional.is_some() && x == ZERO => {
                println!("entered 0x0 if optional.is_some() ZERO");
                println!(
                    "status_code {:#0x} with optional {:#?} and x {:#0x}",
                    status_code, optional, x
                );
            }
            x if optional.is_none() && x == FULL => {
                println!("entered 0xff if optional.is_none() FULL");
                println!(
                    "status_code {:#0x} with optional {:#?} and x {:#0x}",
                    status_code, optional, x
                );
            }
            x if optional.is_some() && x == FULL => {
                println!("entered 0xff if optional.is_some() FULL");
                println!(
                    "status_code {:#0x} with optional {:#?} and x {:#0x}",
                    status_code, optional, x
                );
            }
            other => {
                println!("entered other");
                println!(
                    "status_code {:#0x} with optional {:#?} other {:#0x}",
                    status_code, optional, other
                );
            }
        }
        */

        use test_constants::{FULL, ZERO};
        println!();
        println!("matching on tuple pattern!");
        match (status_code, optional) {
            (ZERO, None) => {
                println!("entered 0x0 if optional.is_none() ZERO");
                println!(
                    "status_code {:#0x} with optional {:#?} no x",
                    status_code, optional
                );
            }
            (ZERO, Some(x)) => {
                println!("entered 0x0 if optional.is_some() ZERO");
                println!(
                    "status_code {:#0x} with optional {:#?} and x {:#0x}",
                    status_code, optional, x
                );
            }
            (FULL, None) => {
                println!("entered 0xff if optional.is_none() FULL");
                println!(
                    "status_code {:#0x} with optional {:#?} no x",
                    status_code, optional
                );
            }
            (FULL, Some(x)) => {
                println!("entered 0xff if optional.is_some() FULL");
                println!(
                    "status_code {:#0x} with optional {:#?} and x {:#0x}",
                    status_code, optional, x
                );
            }
            other => {
                println!("entered other");
                println!(
                    "status_code {:#0x} with optional {:#?} other {:#?}",
                    status_code, optional, other
                );
            }
        }
    }

    println!();
    println!("Test out result mapping");
    {
        let first_error: Result<u32, String> = Err(format!("First error"));
        let second_error: Result<f32, String> = Err(format!("Second error"));
        let third_error: Result<u64, String> = Err(format!("Third error"));
        let ok: Result<u128, String> = Ok(0xffff);

        // let result = first_error
        //     .or_else(|first| {
        //         println!("Entering first map with {:#?}", first);
        //         second_error
        //     })
        //     .map_err(|second| {
        //         println!("Entering second map with {:#?}", second);
        //         third_error
        //     })
        //     .map_err(|third| {
        //         println!("Entering third map with {:#?}", third);
        //         ok
        //     });

        // TODO(alex) 2021-04-04: Is this the only way to chain map on error? Every new state that
        // has a `connection_id` would have to be inserted here.
        //
        // ADD(alex): 2021-04-04: Instead of chaining `match`es, just add a component indicating
        // that the `Host` has a connection id! Simple and efficient.
        let result = match first_error {
            Ok(val) => Some(val as u128),
            Err(first) => {
                println!("Entering first match with {:#?}", first);
                match second_error {
                    Ok(val) => Some(val as u128),
                    Err(second) => {
                        println!("Entering second match with {:#?}", second);
                        match third_error {
                            Ok(val) => Some(val as u128),
                            Err(third) => {
                                println!("Entering third match with {:#?}", third);
                                match ok {
                                    Ok(val) => Some(val as u128),
                                    Err(invalid) => {
                                        println!("Entering fourth match with {:#?}", invalid);
                                        unreachable!()
                                    }
                                }
                            }
                        }
                    }
                }
            }
        };

        // let result = if let Ok(first) = first_error {
        //     println!("Entering first map with {:#?}", first);
        //     Some(first as u128)
        // } else if let Ok(second) = second_error {
        //     println!("Entering second map with {:#?}", second);
        //     Some(second as u128)
        // } else if let Ok(third) = third_error {
        //     println!("Entering third map with {:#?}", third);
        //     Some(third as u128)
        // } else if let Ok(fourth) = ok {
        //     println!("Entering fourth map with {:#?}", fourth);
        //     Some(fourth as u128)
        // } else {
        //     println!("Entering else map");
        //     None
        // };

        println!("Final result is {:#?}", result);
    }
}
