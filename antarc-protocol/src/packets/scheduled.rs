use super::*;

// TODO(alex) [mid] 2021-08-19: Create a derive macro for the `into_packet` implementation, it's the
// same for every kind of packet.
#[derive(Debug, Clone, PartialEq)]
pub struct Scheduled<R: Reliability, Message: Messager> {
    pub packet_id: PacketId,
    pub address: SocketAddr,
    pub time: Duration,
    pub reliability: R,
    pub message: Message,
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum ReliabilityType {
    Reliable,
    Unreliable,
}

pub trait Reliability {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reliable {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreliable {}

impl Reliability for Reliable {}

impl Reliability for Unreliable {}

impl Scheduled<Reliable, ConnectionRequest> {
    pub fn connection_request(packet_id: PacketId, address: SocketAddr, time: Duration) -> Self {
        let reliability = Reliable {};
        let meta = MetaMessage {
            packet_type: CONNECTION_REQUEST,
        };
        let message = ConnectionRequest { meta };

        Self {
            packet_id,
            address,
            time,
            reliability,
            message,
        }
    }

    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, ConnectionRequest> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }
}

impl Scheduled<Reliable, ConnectionAccepted> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, ConnectionAccepted> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }
}

impl Scheduled<Unreliable, DataTransfer> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, DataTransfer> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }
}

impl Scheduled<Reliable, DataTransfer> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, DataTransfer> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }
}

impl Scheduled<Unreliable, Fragment> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, Fragment> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }

    pub(crate) fn new_unreliable_fragment(
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        fragment_index: usize,
        fragment_total: usize,
        time: Duration,
        address: SocketAddr,
    ) -> Self {
        let scheduled = Self {
            packet_id,
            time,
            address,
            reliability: Unreliable {},
            message: Fragment::new(connection_id, payload, fragment_index, fragment_total),
        };

        scheduled
    }
}

impl Scheduled<Reliable, Fragment> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, Fragment> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }

    pub(crate) fn new_reliable_fragment(
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        fragment_index: usize,
        fragment_total: usize,
        time: Duration,
        address: SocketAddr,
    ) -> Self {
        let scheduled = Self {
            packet_id,
            time,
            address,
            reliability: Reliable {},
            message: Fragment::new(connection_id, payload, fragment_index, fragment_total),
        };

        scheduled
    }
}

impl Scheduled<Unreliable, Heartbeat> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, Heartbeat> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }
}

impl Scheduled<Reliable, Heartbeat> {
    pub fn into_packet(
        self,
        sequence: Sequence,
        ack: Ack,
        time: Duration,
    ) -> Packet<ToSend, Heartbeat> {
        let delivery = ToSend {
            id: self.packet_id,
            meta: MetaDelivery {
                time,
                address: self.address,
            },
        };
        let packet = Packet {
            delivery,
            sequence,
            ack,
            message: self.message,
        };

        packet
    }
}

impl Scheduled<Unreliable, DataTransfer> {
    pub(crate) fn new_unreliable_data_transfer(
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        time: Duration,
        address: SocketAddr,
    ) -> Self {
        let scheduled = Self {
            packet_id,
            address,
            time,
            reliability: Unreliable {},
            message: DataTransfer::new(connection_id, payload),
        };

        scheduled
    }
}

impl Scheduled<Reliable, DataTransfer> {
    pub(crate) fn new_reliable_data_transfer(
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        time: Duration,
        address: SocketAddr,
    ) -> Self {
        let scheduled = Self {
            packet_id,
            address,
            time,
            reliability: Reliable {},
            message: DataTransfer::new(connection_id, payload),
        };

        scheduled
    }
}

impl Scheduled<Unreliable, Heartbeat> {
    pub(crate) fn new_unreliable_heartbeat(
        packet_id: PacketId,
        connection_id: ConnectionId,
        time: Duration,
        address: SocketAddr,
    ) -> Self {
        let scheduled = Self {
            packet_id,
            address,
            time,
            reliability: Unreliable {},
            message: Heartbeat::new(connection_id),
        };

        scheduled
    }
}

impl Scheduled<Reliable, Heartbeat> {
    pub(crate) fn new_reliable_heartbeat(
        packet_id: PacketId,
        connection_id: ConnectionId,
        time: Duration,
        address: SocketAddr,
    ) -> Self {
        let scheduled = Self {
            packet_id,
            address,
            time,
            reliability: Reliable {},
            message: Heartbeat::new(connection_id),
        };

        scheduled
    }
}
