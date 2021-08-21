use std::net::SocketAddr;

use crate::{errors::*, packets::*};

// impl<S: ServiceEvent> Into<ProtocolEvent<S>> for ProtocolError {
//     fn into(self) -> ProtocolEvent<S> {
//         ProtocolEvent::Fail(self)
//     }
// }

impl<S: ServiceEvent> From<ProtocolError> for ProtocolEvent<S> {
    fn from(error: ProtocolError) -> Self {
        ProtocolEvent::Fail(error)
    }
}

pub trait ServiceEvent {}

#[derive(Debug)]
pub enum ProtocolEvent<S: ServiceEvent> {
    Fail(ProtocolError),
    DataTransfer {
        connection_id: ConnectionId,
        payload: Payload,
    },
    ServiceEvent(S),
}

/// TODO(alex) [low] 2021-05-23: These separate event types with a common ground is definitely the
/// way to go, but right now they add a bit too much refactoring work, so come back to this once
/// antarc is properly working.
#[derive(Debug, PartialEq, Clone)]
pub enum ServerEvent {
    ConnectionRequest {
        connection_id: ConnectionId,
        remote: SocketAddr,
    },
}

#[derive(Debug, PartialEq, Clone)]
pub enum ClientEvent {
    ConnectionAccepted { connection_id: ConnectionId },
}

impl ServiceEvent for ServerEvent {}
impl ServiceEvent for ClientEvent {}
