pub(super) trait Cookie {}

pub(super) struct EmptyCookie;
pub(super) struct ValueCookie([u8; 32]);

impl Cookie for EmptyCookie {}
impl Cookie for ValueCookie {}
