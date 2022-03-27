use crate::{ContentType, Message};

mod hello;
mod hello_verify;

trait Handshake {
    const HANDSHAKE_TYPE: ContentType;
}

struct HandshakeData {}

impl HandshakeData {
    const MESSAGE_HANDSHAKE: ContentType = ContentType(1);
}

impl Message for HandshakeData {
    const MESSAGE_TYPE: ContentType = Self::MESSAGE_HANDSHAKE;
}
