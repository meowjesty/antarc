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
    fn to_sent(self, footer: Footer) -> Packet<Sent> {
        let sent = Sent { footer };
        Packet {
            header: self.header,
            state: sent,
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

/// NOTE(alex) 2021-05-14: The idea of having `Peer` hold an `Arc<Packet>` and the events holding
/// `Weak<Packet>` doesn't properly work, as changes require taking ownership (move packet into a
/// new state). You'll only be able to take `&mut Packet<State>` from the pointer, but this is not
/// enough to call a function like `to_another_state(self)`.
///
/// If we kept no history of the packets (meaning no `Peer::queued_list`, ...) and use only the
/// events to keep things flowing, then this whole apparatus wouldn't be neccessary, but the history
/// is a bit of a requirement.
#[derive(Debug, PartialEq, Clone)]
struct Peer {
    queued: Vec<Packet<Queued>>,
    sent: Vec<Packet<Sent>>,
    received: Vec<Packet<Received>>,
}

#[derive(Debug, PartialEq)]
enum Event<'x> {
    ReadyToReceive,
    ReadyToSend,
    QueuedPacket {
        packet: Packet<Queued>,
        network: &'x mut Network,
    },
    ToSend {
        packet: Packet<Queued>,
        network: &'x mut Network,
    },
    FailedSendingPacket {
        packet: Packet<Queued>,
    },
    SentPacket {
        packet: Packet<Sent>,
    },
    ReceivedPacket {
        packet: Packet<Received>,
    },
}

#[derive(Debug, PartialEq, Clone)]
struct Socket {
    count: u32,
}

impl Socket {
    const fn is_good(&self) -> bool {
        self.count <= 2
    }

    fn send(&mut self) -> Result<(), String> {
        if self.count > 2 {
            self.count = 0;
            Err("Socket can't write anymore!".to_string())
        } else {
            self.count += 1;
            Ok(())
        }
    }

    fn recv(&mut self) -> Result<(), String> {
        if self.count > 2 {
            self.count = 0;
            Err("Socket can't read anymore!".to_string())
        } else {
            self.count += 1;
            Ok(())
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
struct Network {
    peer: Peer,
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

fn send_packet(packet: Packet<Queued>) -> Result<Packet<Sent>, StateError<Queued>> {
    if packet.header.sequence >= 1 {
        Ok(packet.to_sent(Footer { crc32: 10 }))
    } else {
        Err(StateError {
            fail: "Failed to send!".to_string(),
            previous_state: packet,
        })
    }
}

fn event_handler(event: &Event) {
    match event {
        Event::QueuedPacket { packet, network } => {
            println!("QueuedPacket {:#?} {:#?}", event, packet);
            if network.socket.is_good() {}
        }
        Event::ReceivedPacket { packet } => {
            println!("ReceivedPacket {:#?} {:#?}", event, packet)
        }
        Event::SentPacket { packet } => {
            println!("SentPacket {:#?} {:#?}", event, packet)
        }
        Event::ReadyToReceive => {
            println!("ReadyToReceive {:#?}", event)
        }
        Event::ReadyToSend => {
            println!("ReadyToSend {:#?}", event)
        }
        Event::ToSend { packet, network } => {}
        Event::FailedSendingPacket { packet } => {}
    }
}

fn main() {
    let mut notifier: Notifier<Event> = Notifier::new();
    // TODO(alex) 2021-05-14: How to handle things in these callbacks?
    notifier.register(event_handler);

    let peer = Peer {
        queued: Vec::with_capacity(32),
        sent: Vec::with_capacity(32),
        received: Vec::with_capacity(32),
    };
    let socket = Socket { count: 0 };
    let mut network = Network { peer, socket };

    {
        let mut counter = 0;
        loop {
            if counter % 2 == 0 && network.socket.is_good() {
                notifier.notify(Event::ReadyToReceive);
            } else {
                notifier.notify(Event::ReadyToSend);
            }

            counter += 1;
            if counter > 100 {
                break;
            }
        }
    }
}
