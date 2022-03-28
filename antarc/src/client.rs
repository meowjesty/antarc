use crate::{state::State, Initial, Mode, Preparing, ServiceState};

pub(super) struct Client<S: ServiceState> {
    state: S,
}

impl Client<State<Preparing<Initial>>> {}

impl Mode for Client<State<Preparing<Initial>>> {}
