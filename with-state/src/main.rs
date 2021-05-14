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

#[derive(Debug, PartialEq, Clone)]
struct Peer {
    queued: Vec<Packet<Queued>>,
    sent: Vec<Packet<Sent>>,
    received: Vec<Packet<Received>>,
}

#[derive(Debug, PartialEq, Clone)]
enum Event {
    QueuedPacket { packet: Packet<Queued> },
    FailedSendingPacket { packet: Packet<Queued> },
    SentPacket { packet: Packet<Sent> },
    ReceivedPacket { packet: Packet<Received> },
}

#[derive(Debug, PartialEq, Clone)]
struct Network {
    peer: Peer,
    events: Vec<Event>,
}

fn helper_received() -> Packet<Received> {
    let header = Header {
        sequence: 1,
        ack: 0,
    };
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

fn main() {
    let peer = Peer {
        queued: Vec::with_capacity(32),
        sent: Vec::with_capacity(32),
        received: Vec::with_capacity(32),
    };
    let events = Vec::with_capacity(32);
    let mut network = Network { peer, events };

    {
        let received_packet = Event::ReceivedPacket {
            packet: helper_received(),
        };
        network.events.push(received_packet);

        let received_packet = Event::ReceivedPacket {
            packet: helper_received(),
        };
        network.events.push(received_packet);

        let received_packet = Event::ReceivedPacket {
            packet: helper_received(),
        };
        network.events.push(received_packet);
    }

    {
        let queued_packet = Event::QueuedPacket {
            packet: helper_queued(1),
        };
        network.events.push(queued_packet);

        let queued_packet = Event::QueuedPacket {
            packet: helper_queued(0),
        };
        network.events.push(queued_packet);
    }

    {
        let mut new_events = Vec::with_capacity(32);
        for event in network.events.drain(..) {
            match event {
                Event::QueuedPacket { packet } => match send_packet(packet) {
                    Ok(sent) => {
                        println!("Sent packet {:#?}", sent);
                        let sent_event = Event::SentPacket { packet: sent };
                        new_events.push(sent_event);
                    }
                    Err(fail) => {
                        eprintln!("Failed to send with {:#?}", fail);
                        let fail_sent_event = Event::FailedSendingPacket {
                            packet: fail.previous_state,
                        };
                        new_events.push(fail_sent_event);
                    }
                },
                Event::SentPacket { packet } => {}
                Event::ReceivedPacket { packet } => {}
                Event::FailedSendingPacket { packet } => {
                    eprint!("Retry sending {:#?} ?", packet);
                }
            }
        }

        network.events.append(&mut new_events);
    }
    {
        let mut new_events = Vec::with_capacity(32);
        for event in network.events.drain(..) {
            match event {
                Event::QueuedPacket { packet } => match send_packet(packet) {
                    Ok(sent) => {
                        println!("Sent packet {:#?}", sent);
                        let sent_event = Event::SentPacket { packet: sent };
                        new_events.push(sent_event);
                    }
                    Err(fail) => {
                        eprintln!("Failed to send with {:#?}", fail);
                        let fail_sent_event = Event::FailedSendingPacket {
                            packet: fail.previous_state,
                        };
                        new_events.push(fail_sent_event);
                    }
                },
                Event::SentPacket { packet } => {}
                Event::ReceivedPacket { packet } => {}
                Event::FailedSendingPacket { packet } => {
                    eprint!("Retry sending {:#?} ?", packet);
                }
            }
        }

        network.events.append(&mut new_events);
    }
}
