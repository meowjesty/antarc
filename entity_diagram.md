# Entity diagram

- TODO(alex) 2021-03-23: How do I send data to some address? We're lacking an address here
  to identify both whom `ToSend` and `Received` from whom. There must be a way to relate a `Host`
  and its packets (both sending and receiving).

```mermaid
erDiagram
    Host ||--o{ ToSend : owns
    Host ||--o{ Sent : owns
    Host ||--o{ Acked : owns
    Host ||--o{ Received : owns
    Host ||--o{ Internal : owns
    Host ||--o{ Retrieved : owns
    Host {
        NonZeroU32 sequence_tracker
        u32 ack_tracker
        u16 past_acks_tracker
        Duration rtt
    }

    ToSend ||--|| Packet : is
    Sent||--|| Packet : is
    Acked||--|| Packet : is
    Received||--|| Packet : is
    Internal||--|| Packet : is
    Retrieved||--|| Packet : is

    Packet ||--|| Header : has
    Packet ||--|| Payload : has
    Packet ||--|| Footer : has
    Packet ||--|| Address : "to / from"

    Address {
        SocketAddr address
    }

    Header {
        Sequence sequence
        u32 ack
        u16 past_acks
        StatusCode status_code
    }

    Payload {
        Bytes payload
    }

    Footer {
        NonZeroU32 crc32
    }
```
