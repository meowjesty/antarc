use std::{
    io,
    net::{SocketAddr, UdpSocket},
    num::NonZeroU16,
    time::{Duration, Instant},
    vec::Drain,
};

use antarc_protocol::{packets::*, server::Server, Protocol};
use log::{debug, error, warn};

use super::SendTo;
use crate::net::{NetManager, NetworkResource};

const CHECK_ANTARC_QUEUE: Duration = Duration::from_millis(250);

// TODO(alex) [mid] 2021-06-08: Is this the right approach to reducing duplication?
#[macro_export]
macro_rules! send {
    ($self: expr, $host: expr, $bytes: expr, OnError -> $failed: expr ) => {{
        match $self.network.udp_socket.send_to(&$bytes, $host.address) {
            Ok(num_sent) => {
                debug_assert!(num_sent > 0);
                $host.after_send();
            }
            Err(fail) => {
                if fail.kind() == io::ErrorKind::WouldBlock {
                    warn!("Would block on send_to {:?}", fail);
                    $self.network.writable = false;
                }

                // NOTE(alex): Cannot use `bytes` here (or in any failure event), as
                // it could end up being a duplicated packet, sequence and ack are
                // only incremented when send is successful.
                // $self.event_system.failures.push($failed);

                break;
            }
        }
    }};
}

impl NetManager<Server> {
    pub fn new_server(address: &SocketAddr) -> Self {
        let protocol = Protocol::new_server();
        let net_manager = NetManager::new(address, protocol);
        net_manager
    }

    // TODO(alex) 2021-05-23: Allow the user to specify the destination of these messages, we then
    // check if they're in the `connected` list, and schedule the packets as normal.
    pub fn schedule(&mut self, message: Vec<u8>) -> PacketId {
        todo!()
    }

    // TODO(alex) 2021-05-17: It's probably a good idea to start working on this before going
    // further, to validate that the ideas I had so far are working. Make this the focus.
    pub fn tick(&mut self) -> Result<usize, String> {
        todo!()
    }

    // TODO(alex) [vlow] 2021-06-13: It might be a good idea to return a bit more information than
    // just `ConnectionId`, such as a sequence, or maybe time received, so that the user may know
    // which payload is the "freshest".
    pub fn retrieve(&mut self) -> Drain<(ConnectionId, Vec<u8>)> {
        debug!("Retrieve for server.");
        todo!()
    }
}
