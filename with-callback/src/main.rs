pub struct Notifier<E> {
    subscribers: Vec<Box<dyn Fn(&E)>>,
}

impl<E> Notifier<E> {
    pub fn new() -> Notifier<E> {
        Notifier {
            subscribers: Vec::new(),
        }
    }

    pub fn register<F>(&mut self, callback: F)
    where
        F: 'static + Fn(&E),
    {
        self.subscribers.push(Box::new(callback));
    }

    pub fn notify(&self, event: E) {
        for callback in &self.subscribers {
            callback(&event);
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
struct Header {
    sequence: u32,
    ack: u32,
}

#[derive(Debug, PartialEq, Clone)]
struct Footer {
    crc32: u32,
}

#[derive(Debug, PartialEq, Clone)]
struct Queued {}

impl Packet<Queued> {
    fn to_encoded(self, crc32: u32) -> Packet<Encoded> {
        let footer = Footer { crc32 };
        let bytes = vec![5; 5];
        let state = Encoded { footer, bytes };

        Packet {
            header: self.header,
            state,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
struct Encoded {
    footer: Footer,
    bytes: Vec<u8>,
}

impl Packet<Encoded> {
    fn to_sent(self) -> Packet<Sent> {
        let state = Sent {
            footer: self.state.footer,
        };

        Packet {
            header: self.header,
            state,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
struct Sent {
    footer: Footer,
}

#[derive(Debug, PartialEq, Clone)]
struct Received {
    footer: Footer,
}

#[derive(Debug, PartialEq, Clone)]
struct Packet<State> {
    header: Header,
    state: State,
}

#[derive(Debug, PartialEq, Clone)]
struct RequestingConnection;

#[derive(Debug, PartialEq, Clone)]
struct AwaitingConnectionResponse;

#[derive(Debug, PartialEq, Clone)]
struct Connected;

#[derive(Debug, PartialEq, Clone)]
struct Disconnected;

#[derive(Debug, PartialEq, Clone)]
enum ConnectionState {
    RequestingConnection,
    AwaitingConnectionResponse,
    Disconnected,
    Connected,
}

/// NOTE(alex) 2021-05-14: The idea of having `Peer` hold an `Arc<Packet>` and the events holding
/// `Weak<Packet>` doesn't properly work, as changes require taking ownership (move packet into a
/// new state). You'll only be able to take `&mut Packet<State>` from the pointer, but this is not
/// enough to call a function like `to_another_state(self)`.
///
/// If we kept no history of the packets (meaning no `Peer::queued_list`, ...) and use only the
/// events to keep things flowing, then this whole apparatus wouldn't be neccessary, but the history
/// is a bit of a requirement.
#[derive(Debug, PartialEq, Clone)]
struct Connection {
    received: Vec<Packet<Received>>,
    state: ConnectionState,
}

#[derive(Debug, PartialEq, Clone)]
enum EventKind {
    ReadyToReceive,
    ReadyToSend,
    QueuedPacket,
    FailedSendingPacket,
    SentPacket,
    ReceivedPacket,
}

enum Event<'x> {
    ReadyToReceive {
        socket: &'x Socket,
        notifier: &'x mut Notifier<Self>,
    },
    ReadyToSend {
        has_queued: bool,
        socket: &'x Socket,
        notifier: &'x mut Notifier<Self>,
    },
    QueuedPacket {
        packet: Packet<Queued>,
        socket: &'x Socket,
        notifier: &'x mut Notifier<Self>,
    },
    FailedSendingPacket {
        packet: Packet<Encoded>,
    },
    SentPacket {
        packet: Packet<Sent>,
    },
    ReceivedPacket {
        packet: Packet<Received>,
        connection: &'x mut Connection,
    },
}

impl<'x> Event<'x> {
    fn kind(&self) -> EventKind {
        match self {
            Event::ReadyToReceive { .. } => EventKind::ReadyToReceive,
            Event::ReadyToSend { .. } => EventKind::ReadyToSend,
            Event::QueuedPacket { .. } => EventKind::QueuedPacket,
            Event::FailedSendingPacket { .. } => EventKind::FailedSendingPacket,
            Event::SentPacket { .. } => EventKind::SentPacket,
            Event::ReceivedPacket { .. } => EventKind::ReceivedPacket,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
struct Socket {
    count: u32,
}

impl Socket {
    const fn is_good(&self) -> bool {
        self.count <= 2
    }

    fn send(&mut self, packet: &Packet<Encoded>) -> Result<(), String> {
        if self.count > 2 {
            self.count = 0;
            Err("Socket can't write anymore!".to_string())
        } else {
            self.count += 1;
            println!("Sending packet {:#?}", packet);
            Ok(())
        }
    }

    fn recv(&mut self) -> Result<Packet<Received>, String> {
        if self.count > 2 {
            self.count = 0;
            Err("Socket can't read anymore!".to_string())
        } else {
            self.count += 1;
            Ok(helper_received(self.count))
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
struct Network {
    connection: Connection,
    socket: Socket,
}

fn helper_received(sequence: u32) -> Packet<Received> {
    let header = Header { sequence, ack: 0 };
    let received = Received {
        footer: Footer { crc32: 1 },
    };
    let received_packet = Packet {
        header,
        state: received,
    };
    received_packet
}

fn helper_queued(sequence: u32) -> Packet<Queued> {
    let header = Header { sequence, ack: 0 };
    let queued = Queued {};
    let queued_packet = Packet {
        header,
        state: queued,
    };
    queued_packet
}

#[derive(Debug, PartialEq, Clone)]
struct StateError<State> {
    fail: String,
    previous_state: Packet<State>,
}

fn event_handler(event: &Event) {
    match event {
        Event::QueuedPacket {
            packet,
            socket,
            notifier,
        } => {
            println!("QueuedPacketEvent {:#?}", packet);
            let encoded = packet.to_encoded(32);
            match socket.send(&encoded) {
                Ok(_) => {
                    let sent = encoded.to_sent();
                    println!("Packet was sent {:#?}", sent);
                    notifier.notify(Event::SentPacket { packet: sent });
                }
                Err(fail) => {
                    eprintln!("Failed sending packet with {:#?}", fail);
                    notifier.notify(Event::FailedSendingPacket { packet: encoded });
                }
            }
        }
        Event::SentPacket { packet } => {
            println!("SentPacketEvent {:#?}", packet);
        }
        Event::ReceivedPacket { packet, connection } => {
            println!("ReceivedPacketEvent {:#?}", packet);
            connection.received.push(packet);
        }
        Event::FailedSendingPacket { packet } => {
            eprintln!("FailedSendingPacketEvent {:#?}", packet);
        }
        Event::ReadyToReceive { socket, notifier } => {
            println!("ReadyToReceiveEvent");
            if let Ok(received) = socket.recv() {
                println!("recv {:#?}", received);
                notifier.notify(Event::ReceivedPacket {
                    packet: received,
                    connection: (),
                });
            }
        }
        Event::ReadyToSend {
            has_queued,
            socket,
            notifier,
        } => {
            println!("ReadyToSendEvent");
            if has_queued == false {
                println!("Have no packet queued, send hearbeat to change socket!");
                let packet = helper_queued(15);
                notifier.notify(Event::QueuedPacket {
                    packet,
                    socket,
                    notifier,
                });
            }
        }
    }
}

fn main() {
    let mut notifier: Notifier<Event> = Notifier::new();
    // TODO(alex) 2021-05-14: How to handle things in these callbacks?
    notifier.register(event_handler);

    let connection = Connection {
        received: Vec::with_capacity(32),
        state: ConnectionState::Disconnected,
    };
    let socket = Socket { count: 0 };
    let mut network = Network { connection, socket };

    {
        for i in 0..12 {
            let has_queued = i % 3 == 0;

            if i % 2 == 0 && network.socket.is_good() {
                notifier.notify(Event::ReadyToReceive);
            } else if network.socket.is_good() {
                notifier.notify(Event::ReadyToSend {
                    has_queued,
                    socket: network.socket,
                    notifier,
                });
            } else {
                network.socket.count = 0;
            }
        }
    }
}
