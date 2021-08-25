use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct PartialDecode {
    pub buffer: Vec<u8>,
    pub buffer_position: usize,
    pub packet_type: PacketType,
    pub sequence: Sequence,
    pub ack: Ack,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecodedForClient {
    ConnectionAccepted {
        packet: Packet<Received, ConnectionAccepted>,
    },
    DataTransfer {
        packet: Packet<Received, DataTransfer>,
    },
    Fragment {
        packet: Packet<Received, Fragment>,
    },
    Heartbeat {
        packet: Packet<Received, Heartbeat>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecodedForServer {
    ConnectionRequest {
        packet: Packet<Received, ConnectionRequest>,
    },
    DataTransfer {
        packet: Packet<Received, DataTransfer>,
    },
    Fragment {
        packet: Packet<Received, Fragment>,
    },
    Heartbeat {
        packet: Packet<Received, Heartbeat>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecodedCommon {
    ConnectionRequest {
        packet: Packet<Received, ConnectionRequest>,
    },
    ConnectionAccepted {
        packet: Packet<Received, ConnectionAccepted>,
    },
    DataTransfer {
        packet: Packet<Received, DataTransfer>,
    },
    Fragment {
        packet: Packet<Received, Fragment>,
    },
    Heartbeat {
        packet: Packet<Received, Heartbeat>,
    },
}

impl TryFrom<DecodedCommon> for DecodedForServer {
    type Error = ProtocolError;

    fn try_from(common: DecodedCommon) -> Result<Self, Self::Error> {
        match common {
            DecodedCommon::ConnectionRequest { packet } => {
                Ok(DecodedForServer::ConnectionRequest { packet })
            }
            DecodedCommon::ConnectionAccepted { packet } => Err(ProtocolError::InvalidPacketType(
                packet.message.meta.packet_type,
            )),
            DecodedCommon::DataTransfer { packet } => Ok(DecodedForServer::DataTransfer { packet }),
            DecodedCommon::Fragment { packet } => Ok(DecodedForServer::Fragment { packet }),
            DecodedCommon::Heartbeat { packet } => Ok(DecodedForServer::Heartbeat { packet }),
        }
    }
}

impl TryFrom<DecodedCommon> for DecodedForClient {
    type Error = ProtocolError;

    fn try_from(common: DecodedCommon) -> Result<Self, Self::Error> {
        match common {
            DecodedCommon::ConnectionRequest { packet } => Err(ProtocolError::InvalidPacketType(
                packet.message.meta.packet_type,
            )),
            DecodedCommon::ConnectionAccepted { packet } => {
                Ok(DecodedForClient::ConnectionAccepted { packet })
            }
            DecodedCommon::DataTransfer { packet } => Ok(DecodedForClient::DataTransfer { packet }),
            DecodedCommon::Fragment { packet } => Ok(DecodedForClient::Fragment { packet }),
            DecodedCommon::Heartbeat { packet } => Ok(DecodedForClient::Heartbeat { packet }),
        }
    }
}
