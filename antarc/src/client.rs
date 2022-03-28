use crate::{state::State, Initial, Preparing, Service, Timer};

pub(super) struct Client<Status> {
    status: Status,
}

impl Client<State<Preparing<Initial>>> {}

impl<T: Timer> Service<Client<State<Preparing<Initial>>>, T> {
    pub fn new(timer: T) -> Self {
        todo!()
    }
}
