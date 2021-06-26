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
struct Scheduled {}

impl Packet<Scheduled> {
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
    scheduled: Vec<Packet<Scheduled>>,
    sent: Vec<Packet<Sent>>,
    received: Vec<Packet<Received>>,
}

#[derive(Debug, PartialEq, Clone)]
struct Network {
    peer: Peer,
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

fn helper_scheduled(sequence: u32) -> Packet<Scheduled> {
    let header = Header { sequence, ack: 0 };
    let scheduled = Scheduled {};
    let scheduled_packet = Packet {
        header,
        state: scheduled,
    };
    scheduled_packet
}

#[derive(Debug, PartialEq, Clone)]
struct StateError<State> {
    fail: String,
    previous_state: Packet<State>,
}

fn send_packet(packet: Packet<Scheduled>) -> Result<Packet<Sent>, StateError<Scheduled>> {
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
        scheduled: Vec::with_capacity(32),
        sent: Vec::with_capacity(32),
        received: Vec::with_capacity(32),
    };
    let mut network = Network { peer };

    {
        let received_packet = helper_received();
        network.peer.received.push(received_packet);

        let received_packet = helper_received();
        network.peer.received.push(received_packet);

        let received_packet = helper_received();
        network.peer.received.push(received_packet);
    }

    {
        let scheduled_packet = helper_scheduled(1);
        network.peer.scheduled.push(scheduled_packet);

        let scheduled_packet = helper_scheduled(0);
        network.peer.scheduled.push(scheduled_packet);
    }

    {
        let failed = {
            let mut scheduled_iter = network.peer.scheduled.into_iter();
            let failed: Option<StateError<Scheduled>> = loop {
                if let Some(scheduled) = scheduled_iter.next() {
                    match send_packet(scheduled) {
                        Ok(sent) => {
                            println!("Sent packet {:#?}", sent);
                            network.peer.sent.push(sent);
                        }
                        Err(fail) => {
                            eprintln!("Failed to send with {:#?}", fail);
                            break Some(fail);
                        }
                    }
                }
            };
            failed
        };

        eprintln!("Failed state {:#?}", failed);

        if let Some(fail) = failed {
            // NOTE(alex) 2021-05-14: Another failure, `into_iter` will take the values like I want,
            // but then the `scheduled` list is moved out completely. To make this work, it would need
            // a `iter_mut` at most, but this isn't good enough to take the packet and move it into
            // the next state. This makes total sense, I was testing it out to see if there was a
            // way to make this work.
            // network.peer.scheduled.push(fail.previous_state);
        }
    }
}
