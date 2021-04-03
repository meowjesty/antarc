#![feature(write_all_vectored)]

use std::{
    convert::{TryFrom, TryInto},
    future::Future,
    io::{BufRead, Cursor, IoSlice, Read, Write},
    mem,
    net::{SocketAddr, UdpSocket},
    num::NonZeroU32,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
    thread,
    time::{Duration, Instant},
};

use antarc::{net::NetManager, server::Server};
use hecs::{Entity, With, Without, World};

fn client_main() {
    let server_addr: SocketAddr = "127.0.0.1:7777".parse().unwrap();
    let client_addr: SocketAddr = "127.0.0.1:8888".parse().unwrap();
    let mut net_client = NetManager::new_client(&client_addr);

    // .await ?
    // net_client.connect(&server_addr);
    // TODO(alex): 2021-02-20: This is part of the network manager side of things. When getting a
    // `Peer<Client<Connecting>>`, it keeps ticking until the client is actually connected to the
    // server. The user API won't be calling these inner `tick` functions, it'll call a
    // `NetworkManager.tick`.
    // let mut net_client = client_connecting.connected();
    let world_state = vec![0x0; 32];

    'running: loop {
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

        let data = vec![0x1; 32];
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
    // let server_addr = "127.0.0.1:7777".parse().unwrap();
    let client_addr = "127.0.0.1:8888";

    // let mut server: NetManager<Server> = NetManager::<Server>::new_server(&server_addr);
    let world_state = vec![0x0; 32];

    'running: loop {
        // TODO(alex) 2021-01-27: This keeps the server running by:
        // 1. sending hearbeat packets to clients;
        // 2. putting data packets received in the received data storage;
        // TODO(alex) 2021-01-27: Right now we accept every connection, but in the future we want
        // a ban list, so accepting connections will be part of the protocol API.
        // let new_connections? = server.connection_requests();
        // server.deny_connection(new_connections[1]);
        // server.tick();

        let new_world_state = vec![0x1; 32];
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
        let player_1 = 1;
        // server.ban_host(player_1);
        // }
        // let world_state_changes = world_state.delta().serialize();
        // TODO(alex) 2021-01-27: Work priority in the future, right now I would rather not
        // deal with handling this.
        // server.send_with_priority(Priority::High, id, dead_player_world_state);
        let world_state_changes = vec![0x2; 32];
        // server.enqueue(world_state_changes); // .await
    }
}

#[derive(Debug)]
struct NumberFuture {
    /// TODO(alex) 2021-03-05: Is this the only way to have mutable state in the `poll`?
    value: i32,
    waker: Option<Arc<Mutex<Waker>>>,
}

impl NumberFuture {
    fn new() -> Self {
        Self {
            value: 15,
            waker: None,
        }
    }
    async fn pair(&self) -> f32 {
        1.0
    }
}

impl Future for NumberFuture {
    type Output = f32;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        // First, if this is the first time the future is called, spawn the
        // timer thread. If the timer thread is already running, ensure the
        // stored `Waker` matches the current task's waker.
        if let Some(waker) = &self.waker {
            let mut waker = waker.lock().unwrap();

            // Check if the stored waker matches the current task's waker.
            // This is necessary as the `Delay` future instance may move to
            // a different task between calls to `poll`. If this happens, the
            // waker contained by the given `Context` will differ and we
            // must update our stored waker to reflect this change.
            if !waker.will_wake(cx.waker()) {
                *waker = cx.waker().clone();
            }
        } else {
            // This is the first time `poll` is called.
            let value = self.value;
            let waker = Arc::new(Mutex::new(cx.waker().clone()));
            self.waker = Some(waker.clone());

            thread::spawn(move || {
                if value % 2 != 0 {
                    thread::sleep(Duration::from_millis(150));
                    println!("Value {:?} not pair yet.", value)
                }

                let waker = waker.lock().unwrap();
                waker.wake_by_ref();
            });
        }
        // Once the waker is stored and the timer thread is started, it is
        // time to check if the delay has completed. This is done by
        // checking the current instant. If the duration has elapsed, then
        // the future has completed and `Poll::Ready` is returned.
        if self.value % 2 == 0 {
            println!("Value is pair now {:?}", self.value);
            Poll::Ready(self.value as f32)
        } else {
            self.value += 1;

            // The duration has not elapsed, the future has not completed so
            // return `Poll::Pending`.
            //
            // The `Future` trait contract requires that when `Pending` is
            // returned, the future ensures that the given waker is signalled
            // once the future should be polled again. In our case, by
            // returning `Pending` here, we are promising that we will
            // invoke the given waker included in the `Context` argument
            // once the requested duration has elapsed. We ensure this by
            // spawning the timer thread above.
            //
            // If we forget to invoke the waker, the task will hang
            // indefinitely.
            Poll::Pending
        }
    }
}

fn foo() -> std::io::Result<()> {
    let mut buffer = vec![0; 128];
    let mut buffer2 = vec![0; 128];
    let timer = Instant::now();
    println!("create task");
    let task = async_std::task::spawn(async move {
        println!("begin task");
        let socket = async_std::net::UdpSocket::bind("127.0.0.1:7777")
            .await
            .unwrap();
        println!("socket bound");
        println!("before receive");
        let read =
            async_std::future::timeout(Duration::from_millis(1000), socket.recv_from(&mut buffer))
                .await;
        println!("after received, got {:?}", read);
        // println!("{:?}", read_with.await);
        // let foo = socket.recv_from(&mut buffer2).await;
        println!("end task");
        32
    });
    println!("task created");

    println!("create block");
    let block = async_std::task::block_on(async {
        println!("await task");
        let x = task.await;
        println!("After task");
    });
    println!("block created");

    let mut x = false;
    let mut y = 0;
    let foo = loop {
        break match x {
            true => true,
            false => {
                println!("x is false, continue");
                println!("y {:?}", y);
                y += 1;
                if y > 3000 {
                    x = true;
                }
                continue;
            }
        };
    };

    println!("Foo is {:?}", foo);

    Ok(())

    /*
    async_std::block_on(async {
        let manager = NumberFuture::new();
        manager.pair().await;
        manager.await;

        Ok(())
    })
    */
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

fn main() {
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
    for (packet_id, (destination)) in world.query::<(&Destination,)>().iter() {
        println!("Destination {:#?} {:#?}", packet_id, destination);
    }

    println!("Packets with source");
    for (packet_id, (source)) in world.query::<(&Source,)>().iter() {
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
        let status_code: u16 = 0xff;
        let optional: Option<u16> = Some(0xffff);

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
    }
}
