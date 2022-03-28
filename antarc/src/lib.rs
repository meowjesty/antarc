#![feature(never_type)]

use core::time::Duration;

use application_data::ApplicationData;
use content_type::ContentType;
use limited_vec::LimitedVec;
use record::Record;
use sequence::Sequence;

mod ack_window;
mod application_data;
mod client;
mod content_type;
mod cookie;
mod handshake;
mod length;
mod limited_vec;
mod protocol_version;
mod record;
mod sequence;
mod server;
mod state;

trait Timer {
    fn now() -> Self;
    fn elapsed(&self) -> Duration;
}

struct ReliableQueue {
    records: LimitedVec<Record<Reliable, ApplicationData>>,
}

trait Reliability {}

struct Reliable;
struct Unreliable;

impl Reliability for Reliable {}
impl Reliability for Unreliable {}

trait Message {
    const MESSAGE_TYPE: ContentType;
}

struct Initial {}

struct Preparing<InnerState> {
    preparing: InnerState,
}

struct Awaiting<InnerState> {
    awaiting: InnerState,
}

/// NOTE(alex): The pure API.
struct Service<Mode, T: Timer> {
    local_sequence_tracker: Sequence,
    // TODO(alex) [high] 2022-03-28: Hmm, do I need this here? It's a thing that doesn't exist for
    // some states, for example, `Client<Preparing<Initial>>` has no use whatsoever for this, it
    // is also of questionable use for `Client<Awaiting<HelloVerify>>`, same for
    // `Server<Awaiting<Initial>>`.
    //
    // I'll proceed with the thinking that, if it's a `bool` or an `Option`, then it doesn't belong
    // at the place, and should become a part of the struct that cares about the thing. This means
    // that some of these objects will probably be passed in the inner workings of the
    // implementation, such as in `Initial`?
    //
    // This also creates a differentiation between some inner parts of `Client` / `Server`, as if
    // this value was put inside `Initial`, then it would also be "wrong", as the `Server<Initial>`
    // doesn't care about it (it's optional). But it can't be part of `Preparing` either, as the
    // `Client` won't have a value to fill this with until the `Server` sends a `HelloVerify`
    // request, in which the `Client` starts to keep track of "remote" data parts.
    remote_sequence_tracker: Option<Sequence>,
    timer: T,
    mode: Mode,
}
