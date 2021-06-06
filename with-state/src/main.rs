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

#[allow(dead_code)]
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

#[derive(Debug, PartialEq, Clone)]
enum Event {
    ReadyToReceive,
    ReadyToSend,
    QueuedPacket { packet: Packet<Queued> },
    FailedSendingPacket { packet: Packet<Encoded> },
    SentPacket { packet: Packet<Sent> },
    ReceivedPacket { packet: Packet<Received> },
}

impl Event {
    fn kind(&self) -> EventKind {
        match self {
            Event::ReadyToReceive => EventKind::ReadyToReceive,
            Event::ReadyToSend => EventKind::ReadyToSend,
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
    events: Vec<Event>,
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

fn main() {
    let connection = Connection {
        received: Vec::with_capacity(32),
        state: ConnectionState::Disconnected,
    };
    let events = Vec::with_capacity(32);
    let socket = Socket { count: 0 };
    let mut network = Network {
        connection,
        events,
        socket,
    };

    {
        network.events.push(Event::QueuedPacket {
            packet: helper_queued(1),
        });
        network.events.push(Event::QueuedPacket {
            packet: helper_queued(2),
        });

        let mut new_events = Vec::with_capacity(32);
        for i in 0..12 {
            if i % 2 == 0 && network.socket.is_good() {
                network.events.push(Event::ReadyToReceive);
            } else if network.socket.is_good() {
                network.events.push(Event::ReadyToSend);
            } else {
                network.socket.count = 0;
            }

            let has_queued = network
                .events
                .iter()
                .any(|event| event.kind() == EventKind::QueuedPacket);
            for event in network.events.drain(..) {
                match event {
                    Event::QueuedPacket { packet } => {
                        println!("QueuedPacketEvent {:#?}", packet);
                        let encoded = packet.to_encoded(32);
                        match network.socket.send(&encoded) {
                            Ok(_) => {
                                let sent = encoded.to_sent();
                                println!("Packet was sent {:#?}", sent);
                                new_events.push(Event::SentPacket { packet: sent });
                            }
                            Err(fail) => {
                                eprintln!("Failed sending packet with {:#?}", fail);
                                new_events.push(Event::FailedSendingPacket { packet: encoded })
                            }
                        }
                    }
                    Event::SentPacket { packet } => {
                        println!("SentPacketEvent {:#?}", packet);
                    }
                    Event::ReceivedPacket { packet } => {
                        println!("ReceivedPacketEvent {:#?}", packet);
                        network.connection.received.push(packet);
                    }
                    Event::FailedSendingPacket { packet } => {
                        eprintln!("FailedSendingPacketEvent {:#?}", packet);
                    }
                    Event::ReadyToReceive => {
                        println!("ReadyToReceiveEvent");
                        if let Ok(received) = network.socket.recv() {
                            println!("recv {:#?}", received);
                            new_events.push(Event::ReceivedPacket { packet: received });
                        }
                    }
                    Event::ReadyToSend => {
                        println!("ReadyToSendEvent");
                        if has_queued == false {
                            println!("Have no packet queued, send hearbeat to change socket!");
                            new_events.push(Event::QueuedPacket {
                                packet: helper_queued(i * 10 + 10),
                            });
                        }
                    }
                }
            }

            // TODO(alex) 2021-05-14: This approach requires 2 lists, one that is being drained, and
            // a secondary that will be moved into the drained list with new values.
            network.events.append(&mut new_events);
        }
    }
}
