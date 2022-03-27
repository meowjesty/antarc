use super::{Handshake, hello::Hello};
use crate::{cookie::ValueCookie, ContentType};

struct HelloVerify {
    cookie: ValueCookie,
}

impl Handshake for HelloVerify {
    const HANDSHAKE_TYPE: ContentType = Self::HANDSHAKE_HELLO_VERIFY;
}

impl HelloVerify {
    const HANDSHAKE_HELLO_VERIFY: ContentType =
        ContentType(Hello::HANDSHAKE_HELLO_WITH_COOKIE.0 + 1);
}
