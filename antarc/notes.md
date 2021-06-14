# Notes on the protocol

## Handshake

| (state)                 |       Client        |       |       Server       | (state)             |
| :---------------------- | :-----------------: | :---: | :----------------: | :------------------ |
| (requesting connection) |  send conn request  | --->  |         ()         | ()                  |
| (awaiting response)     |         ...         | ----  | recv conn request  | (requesting conn)   |
| (awaiting response)     |         ...         | <---  | send conn accepted | (awaiting conn ack) |
| (partial connected)     | recv conn accepted. | ----  |        ...         | (awaiting conn ack) |
| (partial connected)     |  send data + ack.   | --->  |        ...         | (awaiting conn ack) |
| (partial connected)     |         ...         | ----  |      recv ack      | (connected)         |
| (partial connected)     |         ...         | <---  |  send data + ack   | (connected)         |
| (connected)             |   recv data + ack   | ----  |        ...         | (connected)         |
