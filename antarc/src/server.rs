use crate::{state::State, Awaiting, Initial, Mode, Preparing, ServiceState};

pub(super) struct Server<S: ServiceState> {
    state: S,
}

impl Server<State<Awaiting<Initial>>> {}

impl Mode for Server<State<Awaiting<Initial>>> {}

// TODO(alex) [high] 2022-03-28: This should be impossible (compile-time error).
impl Mode for Server<State<Preparing<Initial>>> {}
