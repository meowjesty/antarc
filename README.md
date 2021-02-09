# Antarc UDP protocol

**Client**:

```mermaid
graph TB
    client --> connect;

    client --> tick;
    subgraph Tick
    tick --> receive[[recv]];
    receive --> enqueue_request[[enqueue_request]];
    tick --> received{requests?};
    received --> |DataTransfer| received_data((Vec<u8>));
    received --> |Heartbeat| heartbeat((Heartbeat));
    end
    received_data & heartbeat --> |Ack| enqueue_response[[enqueue_response]];

    client --> send;
    send --> enqueue_response;

    client --> retrieve;
    retrieve --> get_received_packets{received<br/>data transfer<br/> packets?};
    get_received_packets --> |Some| received_packets[(received packets)];
    received_packets --> data((Vec<Vec<u8>>));
    get_received_packets --> |None| empty((empty));
```

```mermaid
classDiagram
    %% Items marked with * are part of the raw `Header`, but don't exist in the struct itself.
    class RawHeader {
        *crc32: u32,
        *protocol_id: u32,
        connection_id: u16,
        sequence: u32,
        ack: u32,
        past_acks: u16,
        *kind: u16,
        encode()
        decode()
    }

    class RawPacket {
        header: RawHeader,
        body: Vec<u8>,
    }

    RawPacket *-- RawHeader

    class Metadata {
        time: Duration,
    }

    class DataPacket {
        raw: RawPacket,
        kind() u16
    }

    class HeartbeatPacket {
        raw: RawPacket,
        kind() u16
    }

    class AckPacket {
        raw: RawPacket,
        kind() u16
    }

    %% Ignore this for now.
    class FragmentPacket {
        raw: RawPacket,
        kind() u16
    }

    DataPacket *-- RawPacket
    AckPacket *-- RawPacket
    HeartbeatPacket *-- RawPacket
    FragmentPacket *-- RawPacket

    class Packet {
        <<interface>>
    }

    Packet o-- RawPacket

    %% How do I achieve this? The protocol may receive any kind of packet, trait object perhaps?
    %% Which of these 3 ways is the best one? Probably the third one, as I want to avoid dynamic
    %% stuff as much as possible, and if each struct will retain ownership over the packet anyways
    %% there's no need to heap alloc it. References will be just fine, and changes of state will
    %% be implemented as moves into a new state struct.
    class Received {
        *packet: Box<Packet>
        *packet: Box<Packet>
        %% Should it copy the `[u8]` packet data when passing it back to the client? I want to keep
        %% track of everything inside the packets, and only release things from memory when the user
        %% requests it (or it hits some MAX_PACKETS number), but this means that `retrieve` will
        %% have to copy the data content and return this copy. Is there a way of returning just
        %% a reference? It'll probably be fine, as I'm keeping these packets alive for a while, but
        %% will be a bit finnicky, for the state transition will move the packet data, requiring
        %% the reference return to be done after this move.
        *packet: Packet
    }

    class Sent {
        packet: Packet
    }

    class ToSend {
        packet: Packet
    }

    class Acked {
        packet: Packet
    }

    %% There must be a difference between packets received and packets that were already read by
    %% the user, this is the way to mark these packets.
    class Retrieved {
        packet: Packet
    }

    Received *-- Packet
    Sent *-- Packet
    ToSend *-- Packet
    Acked *-- Packet
    Retrieved *-- Packet
```

**Server**:

```rust
let mut players = PlayerClients::new(4);
let mut server_manager = Antarc::new(Server);
server_manager.listen();

// TODO(alex) 2021-01-26: Should we have a `dispatch` and treat `send`, `broadcast` and so on as
// enqueue operations (not immediately executed)? Like having a `server_manager.update()` function
// that goes into each host sending the packets that are queued.
loop {
    let (&data, &from) = server_manager.recv().await;
    if from.id == players[1].id && players[1].is_out_of_sync() {
        let game_state = game_manager.full_state();
        let _ = server_manager.send(SyncPlayer(game_state), from)?;
    } else {
        let _ = server_manager.broadcast(PlayerShot(players[1]))?;
    }
}
```
