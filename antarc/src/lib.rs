use application_data::ApplicationData;
use content_type::ContentType;
use limited_vec::LimitedVec;
use record::Record;

mod ack_window;
mod application_data;
mod content_type;
mod cookie;
mod handshake;
mod limited_vec;
mod protocol_version;
mod record;
mod sequence;

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
