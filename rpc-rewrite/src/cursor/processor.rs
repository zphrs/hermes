pub mod buffers;
mod hybrid;
pub mod loopback;
pub mod processor_fut;
pub mod transition;

use crate::io::Connection;
pub use buffers::Buffers;
pub use processor_fut::ProcessorFut;
use std::marker::PhantomData;

pub struct Processor<State, Role, RootMethod, C: Connection, Handler> {
    connection: C,
    handler: Handler,
    _marker: PhantomData<(State, Role, RootMethod)>,
}

impl<State, Role, RootMethod, C: Connection, Handler>
    Processor<State, Role, RootMethod, C, Handler>
{
    pub(crate) fn new(connection: C, handler: Handler) -> Self {
        Self {
            handler,
            connection,
            _marker: PhantomData,
        }
    }
}
