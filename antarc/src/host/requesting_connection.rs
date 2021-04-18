use std::{net::UdpSocket, time::Instant};

use hecs::{With, Without, World};

use super::RequestingConnection;
use crate::{
    host::Address,
    packet::{
        header::Header, ConnectionRequest, DataTransfer, Internal, Packet, Received, Sequence,
    },
};

pub(crate) fn system_new_host_handler(world: &mut World) {}
