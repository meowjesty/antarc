use application_data::ApplicationData;
use client::Client;
use content_type::ContentType;
use limited_vec::LimitedVec;
use record::Record;
use server::Server;
use state::State;

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

trait InnerState {}

struct Initial {}
impl InnerState for Initial {}

trait Status {}

impl Status for Preparing<Initial> {}
impl Status for Awaiting<Initial> {}

struct Preparing<InnerState> {
    service_state: InnerState,
}

struct Awaiting<InnerState> {
    service_state: InnerState,
}

struct Service<Mode> {
    mode: Mode,
}

impl Service<Client<State<Preparing<Initial>>>> {}
impl Service<Server<State<Preparing<Initial>>>> {}
