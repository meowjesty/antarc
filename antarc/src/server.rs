use crate::{state::State, Awaiting, Initial, Preparing, Service, Timer};

pub(super) struct Server<Status> {
    status: Status,
}

impl Server<State<Awaiting<Initial>>> {}

impl<T: Timer> Service<Server<State<Awaiting<Initial>>>, T> {
    pub fn new(timer: T) -> Self {
        todo!()
    }
}

// TODO(alex) [high] 2022-03-28: This should be impossible (compile-time error).
//
// I'm removing traits (for now) for prototyping reasons, but they should be added to restrict the
// creation of invalid `impl`s.
impl Server<State<Preparing<Initial>>> {
    const _SHOULD_BE_IMPOSSIBLE: ! = unimplemented!();
}
