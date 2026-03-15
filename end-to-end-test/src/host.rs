pub(crate) mod io;
pub mod net;
pub mod os_shim;

use std::{
    cell::RefCell,
    fmt::{Debug, from_fn},
    pin::Pin,
    time::Duration,
};
use tokio::task::AbortHandle;
use tracing::warn;

use crate::sim::machine::{BasicMachine, HasNic, Machine, MachineId};

pub(crate) type Software<'a> = Box<dyn Fn() -> Pin<Box<dyn Future<Output = Result>>> + 'static>;
pub(crate) type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Host {
    entrypoint: Software<'static>,
    handle: RefCell<Option<AbortHandle>>,
    inner_machine: BasicMachine,
}

impl Debug for Host {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Host")
            .field(&from_fn(|f| write!(f, "{}", &self.id())))
            .finish()
    }
}

impl Host {
    pub fn new<F, Fut>(bufsize: usize, software: F) -> Self
    where
        F: Fn() -> Fut + 'static,
        Fut: Future<Output = Result> + 'static,
    {
        let software: Software = Box::new(move || Box::pin(software()));
        let out = Host {
            entrypoint: software,
            handle: None.into(),
            inner_machine: BasicMachine::new(bufsize),
        };
        out.start();
        out
    }

    pub fn inner(&self) -> &BasicMachine {
        &self.inner_machine
    }

    // entrypoint here is a fn to allow for rebooting a host
    #[allow(clippy::async_yields_async)]
    pub fn start(&self) {
        // spawn_local immediately completes, adding the spawned task to
        // the LocalSet
        if self.handle.borrow().is_some() {
            warn!("tried to start an already running task");
            return;
        };
        let handle = self.inner_machine.spawn_local((self.entrypoint)());

        *self.handle.borrow_mut() = Some(handle);
    }

    pub fn stop(&self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        } else {
            warn!("tried to stop a task that is already stopped")
        }
    }
}

impl Machine for Host {
    fn tick(&self, duration: Duration) -> std::result::Result<bool, Box<dyn std::error::Error>> {
        self.inner_machine.tick(duration)?;
        let mut handle = self.handle.borrow_mut();
        if let Some(task) = &*handle {
            if task.is_finished() {
                *handle = None;
            }
        };
        Ok(handle.is_none())
    }

    fn id(&self) -> MachineId {
        self.inner().id()
    }

    fn is_idle(&self) -> bool {
        self.handle.borrow().is_none()
    }
}

impl HasNic for Host {
    fn nic(&self) -> crate::sim::machine::MachineNic {
        self.inner().nic()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{host::Host, sim::machine::Machine};

    #[test]
    fn types() {
        let h = Host::new(10, || async { Ok(()) });
        h.start();
        assert!(!h.is_idle());
        assert!(h.tick(Duration::new(1, 0)).unwrap());
        assert!(h.is_idle());
    }
}
