use crate::*;

#[derive(Debug)]
pub struct ReliabilityHandler<S: ServiceReliability> {
    pub(crate) list_sent_reliable_data_transfer: Vec<Packet<Sent, DataTransfer>>,
    pub(crate) list_sent_reliable_heartbeat: Vec<Packet<Sent, Heartbeat>>,
    pub(crate) list_sent_reliable_fragment: Vec<Packet<Sent, Fragment>>,

    pub(crate) service: S,
}

impl<S: ServiceReliability> ReliabilityHandler<S> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            list_sent_reliable_data_transfer: Vec::with_capacity(capacity),
            list_sent_reliable_heartbeat: Vec::with_capacity(capacity),
            list_sent_reliable_fragment: Vec::with_capacity(capacity),
            service: S::new(capacity),
        }
    }

    pub(crate) fn poll(&mut self, now: Duration) {
        if self
            .list_sent_reliable_data_transfer
            .first()
            .map(|packet| (packet.delivery.meta.time + packet.delivery.ttl > now).then(|| ()))
            .is_some()
        {
            self.list_sent_reliable_data_transfer.remove(0);
        } else if self
            .list_sent_reliable_fragment
            .first()
            .map(|packet| (packet.delivery.meta.time + packet.delivery.ttl > now).then(|| ()))
            .is_some()
        {
            self.list_sent_reliable_fragment.remove(0);
        }

        self.service.poll(now);
    }

    pub(crate) fn retry_reliable_data_transfer(
        &mut self,
        now: Duration,
    ) -> Option<Packet<ToSend, DataTransfer>> {
        if let Some(packet) = self.list_sent_reliable_data_transfer.pop() {
            if packet.delivery.meta.time + now > Duration::from_secs(1000) {
                let meta = MetaDelivery {
                    time: now,
                    address: packet.delivery.meta.address,
                };
                let delivery = ToSend {
                    id: packet.delivery.id,
                    meta,
                };
                let message = DataTransfer {
                    meta: packet.message.meta,
                    connection_id: packet.message.connection_id,
                    payload: packet.message.payload,
                };
                let result = Packet {
                    delivery,
                    sequence: packet.sequence,
                    ack: packet.ack,
                    message,
                };

                return Some(result);
            }
        }

        None
    }

    pub(crate) fn retry_reliable_fragment(
        &mut self,
        now: Duration,
    ) -> Option<Packet<ToSend, Fragment>> {
        if let Some(packet) = self.list_sent_reliable_fragment.pop() {
            if packet.delivery.meta.time + now > Duration::from_secs(1000) {
                let meta = MetaDelivery {
                    time: now,
                    address: packet.delivery.meta.address,
                };
                let delivery = ToSend {
                    id: packet.delivery.id,
                    meta,
                };
                let message = Fragment {
                    meta: packet.message.meta,
                    connection_id: packet.message.connection_id,
                    payload: packet.message.payload,
                    index: packet.message.index,
                    total: packet.message.total,
                };
                let result = Packet {
                    delivery,
                    sequence: packet.sequence,
                    ack: packet.ack,
                    message,
                };

                return Some(result);
            }
        }

        None
    }

    pub(crate) fn retry_reliable_heartbeat(
        &mut self,
        now: Duration,
    ) -> Option<Packet<ToSend, Heartbeat>> {
        if let Some(packet) = self.list_sent_reliable_heartbeat.pop() {
            if packet.delivery.meta.time + now > Duration::from_secs(1000) {
                let meta = MetaDelivery {
                    time: now,
                    address: packet.delivery.meta.address,
                };
                let delivery = ToSend {
                    id: packet.delivery.id,
                    meta,
                };
                let message = Heartbeat {
                    meta: packet.message.meta,
                    connection_id: packet.message.connection_id,
                };
                let result = Packet {
                    delivery,
                    sequence: packet.sequence,
                    ack: packet.ack,
                    message,
                };

                return Some(result);
            }
        }

        None
    }
}
