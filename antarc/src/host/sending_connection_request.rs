use hecs::World;

use super::Host;
use crate::packet::Payload;

pub(crate) fn system_send_connection_request(world: &mut World) {
    for (id, (host, payload)) in &mut world.query::<(&Host, &Payload)>() {}
}
