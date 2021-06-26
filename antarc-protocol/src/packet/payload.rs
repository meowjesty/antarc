#[derive(Debug, Clone, PartialEq, Default, PartialOrd)]
pub struct Payload(pub Vec<u8>);

impl std::ops::Deref for Payload {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for Payload {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Payload {
    pub fn len(&self) -> usize {
        self.0.len()
    }
}
