use core::fmt;
use std::{
    marker::PhantomData,
    net::{SocketAddr, UdpSocket},
    num::{NonZeroU16, NonZeroU32, NonZeroU8},
    time::Instant,
};

use hecs::World;

/// TODO(alex) 2021-02-07: A `Peer<Client>` will connect to the main `Peer<Server>`, and it'll
/// receive information about the other `Peer<Client>` that are connected to the same server. They
/// should probably remain in `disconnected` until there is a server migration (server goes offline
/// for a while), in which case the clients will try to estabilish a new `Peer<Server>` (negotiate
/// using some heuristic to determine which of the known hosts would be best).
///
/// Migration will require having a `into_server` function.
/// Is this the correct abstraction to handle different kinds of peers?
///
/// TODO(alex) 2021-02-15: Can we use the `Header.connection_id` as the `Peer.connected.len()`?
/// The `Client` will always have only 1 item in `Peer.connected`, but the server may have many.
/// Will having a `connection_id: 1`, then disconnecting the peer (after many iterations of the
/// network running), and having a new peer (or the old peer coming back from disconnected) with a
/// `connection_id: 1` cause a conflict? This depends heavily on how we pass the peer identification
/// back to the user, if we rely only on `connection_id` and treat it like this, then the user might
/// think that it's the same peer. This field is a bit more complicated than the `Peer.sequence`, as
/// it kinda has to be unique among many peers, and maybe be the same in case of a
/// disconnect->reconnect peer.
/// ADD(alex) 2021-02-16: Just keep it as part of the Peer, so every connection we check the
/// biggest number, maybe even put a layer above and have a `connection_num` in the network handler.
pub struct NetManager<ClientOrServer> {
    /// TODO(alex): 2021-02-15: `socket` should not be here, I think it belongs in some higher
    /// level manager thingy, as `socket.send` feels weird when used here.
    pub(crate) socket: UdpSocket,
    pub(crate) timer: Instant,
    /// TODO(alex) 2021-02-26: Each `Host` will probably have it's own `buffer`, like the `timer.
    pub(crate) buffer: Vec<u8>,
    pub(crate) client_or_server: ClientOrServer,
    pub(crate) world: World,
}

impl<T: fmt::Debug> fmt::Debug for NetManager<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetManager")
            .field("socket", &self.socket)
            .field("timer", &self.timer)
            .field("buffer", &self.buffer.len())
            .field("client_or_server", &self.client_or_server)
            .field("world", &self.world.len())
            .finish()
    }
}
