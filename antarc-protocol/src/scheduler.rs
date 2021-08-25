use std::net::SocketAddr;

use crate::*;

#[derive(Debug)]
pub struct Scheduler<S: ServiceScheduler> {
    pub(crate) list_scheduled_reliable_data_transfer: Vec<Scheduled<Reliable, DataTransfer>>,
    pub(crate) list_scheduled_unreliable_data_transfer: Vec<Scheduled<Unreliable, DataTransfer>>,
    pub(crate) list_scheduled_reliable_fragment: Vec<Scheduled<Reliable, Fragment>>,
    pub(crate) list_scheduled_unreliable_fragment: Vec<Scheduled<Unreliable, Fragment>>,
    pub(crate) list_scheduled_reliable_heartbeat: Vec<Scheduled<Reliable, Heartbeat>>,
    pub(crate) list_scheduled_unreliable_heartbeat: Vec<Scheduled<Unreliable, Heartbeat>>,

    pub(crate) service: S,
}

impl<S: ServiceScheduler> Scheduler<S> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            list_scheduled_reliable_data_transfer: Vec::with_capacity(capacity),
            list_scheduled_unreliable_data_transfer: Vec::with_capacity(capacity),
            list_scheduled_reliable_fragment: Vec::with_capacity(capacity),
            list_scheduled_unreliable_fragment: Vec::with_capacity(capacity),
            list_scheduled_reliable_heartbeat: Vec::with_capacity(capacity),
            list_scheduled_unreliable_heartbeat: Vec::with_capacity(capacity),
            service: S::new(capacity),
        }
    }

    fn reliable_fragment(&mut self, scheduled: Scheduled<Reliable, Fragment>) {
        self.list_scheduled_reliable_fragment.push(scheduled);
    }

    fn unreliable_fragment(&mut self, scheduled: Scheduled<Unreliable, Fragment>) {
        self.list_scheduled_unreliable_fragment.push(scheduled);
    }

    fn reliable_data_transfer(&mut self, scheduled: Scheduled<Reliable, DataTransfer>) {
        self.list_scheduled_reliable_data_transfer.push(scheduled);
    }

    fn unreliable_data_transfer(&mut self, scheduled: Scheduled<Unreliable, DataTransfer>) {
        self.list_scheduled_unreliable_data_transfer.push(scheduled);
    }

    fn reliable_heartbeat(&mut self, scheduled: Scheduled<Reliable, Heartbeat>) {
        self.list_scheduled_reliable_heartbeat.push(scheduled);
    }

    fn unreliable_heartbeat(&mut self, scheduled: Scheduled<Unreliable, Heartbeat>) {
        self.list_scheduled_unreliable_heartbeat.push(scheduled);
    }

    pub(crate) fn schedule_for_connected_peer(
        &mut self,
        address: SocketAddr,
        payload: Arc<Payload>,
        reliability: ReliabilityType,
        connection_id: ConnectionId,
        packet_id: PacketId,
        time: Duration,
        fragment_index: usize,
        fragment_total: usize,
    ) {
        if fragment_total > 1 {
            debug!("protocol: scheduling fragment");
            self.schedule_fragment(
                reliability,
                packet_id,
                connection_id,
                payload,
                fragment_index,
                fragment_total,
                time,
                address,
            );
        } else {
            debug!("protocol: scheduling data transfer");
            self.schedule_data_transfer(
                reliability,
                packet_id,
                connection_id,
                payload,
                time,
                address,
            );
        }
    }

    pub(crate) fn heartbeat_for_connected_peer(
        &mut self,
        address: SocketAddr,
        reliability: ReliabilityType,
        connection_id: ConnectionId,
        packet_id: PacketId,
        time: Duration,
    ) {
        debug!("protocol: scheduling heartbeat");
        self.schedule_heartbeat(reliability, packet_id, connection_id, time, address);
    }

    fn schedule_fragment(
        &mut self,
        reliability: ReliabilityType,
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        fragment_index: usize,
        fragment_total: usize,
        time: Duration,
        address: SocketAddr,
    ) {
        match reliability {
            ReliabilityType::Reliable => {
                let scheduled: Scheduled<Reliable, Fragment> = Scheduled::new_reliable_fragment(
                    packet_id,
                    connection_id,
                    payload,
                    fragment_index,
                    fragment_total,
                    time,
                    address,
                );

                self.reliable_fragment(scheduled);
            }
            ReliabilityType::Unreliable => {
                let scheduled: Scheduled<Unreliable, Fragment> = Scheduled::new_unreliable_fragment(
                    packet_id,
                    connection_id,
                    payload,
                    fragment_index,
                    fragment_total,
                    time,
                    address,
                );

                self.unreliable_fragment(scheduled);
            }
        }
    }

    fn schedule_data_transfer(
        &mut self,
        reliability: ReliabilityType,
        packet_id: PacketId,
        connection_id: ConnectionId,
        payload: Arc<Payload>,
        time: Duration,
        address: SocketAddr,
    ) {
        match reliability {
            ReliabilityType::Reliable => {
                let scheduled: Scheduled<Reliable, DataTransfer> =
                    Scheduled::new_reliable_data_transfer(
                        packet_id,
                        connection_id,
                        payload,
                        time,
                        address,
                    );

                self.reliable_data_transfer(scheduled);
            }
            ReliabilityType::Unreliable => {
                let scheduled: Scheduled<Unreliable, DataTransfer> =
                    Scheduled::new_unreliable_data_transfer(
                        packet_id,
                        connection_id,
                        payload,
                        time,
                        address,
                    );

                self.unreliable_data_transfer(scheduled);
            }
        }
    }

    fn schedule_heartbeat(
        &mut self,
        reliability: ReliabilityType,
        packet_id: PacketId,
        connection_id: ConnectionId,
        time: Duration,
        address: SocketAddr,
    ) {
        match reliability {
            ReliabilityType::Reliable => {
                let scheduled: Scheduled<Reliable, Heartbeat> =
                    Scheduled::new_reliable_heartbeat(packet_id, connection_id, time, address);

                self.reliable_heartbeat(scheduled);
            }
            ReliabilityType::Unreliable => {
                let scheduled: Scheduled<Unreliable, Heartbeat> =
                    Scheduled::new_unreliable_heartbeat(packet_id, connection_id, time, address);

                self.unreliable_heartbeat(scheduled);
            }
        }
    }
}
