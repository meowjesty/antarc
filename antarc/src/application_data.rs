use crate::{content_type::ContentType, Message};

pub(crate) struct ApplicationData {}

impl ApplicationData {
    const MESSAGE_APPLICATION_DATA: ContentType = ContentType(2);
}

impl Message for ApplicationData {
    const MESSAGE_TYPE: ContentType = Self::MESSAGE_APPLICATION_DATA;
}
