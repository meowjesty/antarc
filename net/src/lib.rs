use antarc::host::{Connected, Connecting, Disconnected};
use antarc::peer::{Client, Peer, Server};

struct ServerNetwork {
    server: Peer<Server>,
}

enum ClientNetwork {
    Disc(Peer<Client<Disconnected>>),
    Conning(Peer<Client<Connecting>>),
    Conn(Peer<Client<Connected>>),
}

struct Network<ServerOrClient> {
    kind: ServerOrClient,
}

impl Network<ClientNetwork> {
    pub fn foo(&self) {
        match self.kind {
            ClientNetwork::Disc(_) => {}
            ClientNetwork::Conning(_) => {}
            ClientNetwork::Conn(_) => {}
        }
    }
}
