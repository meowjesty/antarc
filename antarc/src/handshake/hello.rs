use super::Handshake;
use crate::{
    cookie::{Cookie, EmptyCookie, ValueCookie},
    ContentType,
};

/// 1. Initiates the handshake (client);
/// 2. Awaits a `HelloVerify` (from server) containing the `Cookie`;
/// 3. Replies with `Hello<WithCookie>` or retransmits the same `Hello` on timeout;
pub(super) struct Hello<C: Cookie> {
    cookie: C,
}

impl Handshake for Hello<EmptyCookie> {
    const HANDSHAKE_TYPE: ContentType = Self::HANDSHAKE_HELLO_WITHOUT_COOKIE;
}

impl Handshake for Hello<ValueCookie> {
    const HANDSHAKE_TYPE: ContentType = Self::HANDSHAKE_HELLO_WITH_COOKIE;
}

impl Hello<EmptyCookie> {
    const HANDSHAKE_HELLO_WITHOUT_COOKIE: ContentType = ContentType(1);
}

impl Hello<ValueCookie> {
    pub(super) const HANDSHAKE_HELLO_WITH_COOKIE: ContentType =
        ContentType(Hello::HANDSHAKE_HELLO_WITHOUT_COOKIE.0 + 1);
}
