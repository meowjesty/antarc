use std::{collections::HashMap, time::Duration};

use crate::{packets::*, scheduler::*, *};

pub trait Service {
    type SchedulerType: ServiceScheduler;
    type ReliabilityHandlerType: ServiceReliability;

    const DEBUG_NAME: &'static str;

    fn scheduler(&self) -> &Scheduler<Self::SchedulerType>;
    fn scheduler_mut(&mut self) -> &mut Scheduler<Self::SchedulerType>;

    fn reliability_handler(&self) -> &ReliabilityHandler<Self::ReliabilityHandlerType>;
    fn reliability_handler_mut(&mut self) -> &mut ReliabilityHandler<Self::ReliabilityHandlerType>;

    fn connected(&self) -> &HashMap<ConnectionId, Peer<Connected>>;

    fn sent_data_transfer(
        &mut self,
        packet: Packet<ToSend, DataTransfer>,
        time: Duration,
        reliability: ReliabilityType,
        ttl: Duration,
    );

    fn sent_fragment(
        &mut self,
        packet: Packet<ToSend, Fragment>,
        time: Duration,
        reliability: ReliabilityType,
        ttl: Duration,
    );

    fn sent_heartbeat(
        &mut self,
        packet: Packet<ToSend, Heartbeat>,
        time: Duration,
        reliability: ReliabilityType,
        ttl: Duration,
    );
}

pub trait ServiceScheduler {
    fn new(capacity: usize) -> Self;
}

pub trait ServiceReliability {
    fn new(capacity: usize) -> Self;
    fn poll(&mut self, now: Duration);
}
