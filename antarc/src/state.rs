use crate::{client::Client, server::Server, Awaiting, Initial, Preparing, ServiceState, Status};

pub(super) struct State<S: Status> {
    status: S,
}

impl ServiceState for Client<State<Preparing<Initial>>> {}
impl ServiceState for Server<State<Awaiting<Initial>>> {}
