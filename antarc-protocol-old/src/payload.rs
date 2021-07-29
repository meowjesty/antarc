pub const FRAGMENT_SIZE: usize = 1500;

#[derive(Debug, Clone, PartialEq, Default, PartialOrd)]
pub struct Payload(pub Vec<u8>);

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum Fragment {
    Single(Payload),
    Multiple(Vec<Payload>),
}

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

    pub fn should_fragment(&self) -> bool {
        self.len() > FRAGMENT_SIZE
    }

    pub fn fragment(self) -> Fragment {
        if self.should_fragment() {
            let bytes = self.0;
            let chunks = bytes.chunks(FRAGMENT_SIZE);
            let payloads = chunks
                .map(|chunk| Payload(chunk.to_vec()))
                .collect::<Vec<_>>();

            Fragment::Multiple(payloads)
        } else {
            Fragment::Single(self)
        }
    }
}
