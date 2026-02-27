pub(crate) mod io;
pub mod net;
pub mod os_shim;

use bytes::Bytes;
use std::{
    cell::RefCell,
    fmt::{Debug, from_fn},
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    runtime::Runtime,
    sync::mpsc::{self, Receiver},
    task::{JoinHandle, LocalSet},
};

use crate::sim::machine::{HasNic, Machine, MachineId, MachineNic};

pub(crate) type Software<'a> = Box<dyn Fn() -> Pin<Box<dyn Future<Output = Result>>> + 'a>;
pub(crate) type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Host<'a> {
    pub(crate) id: MachineId,
    pub(crate) nic: MachineNic,
    pub(crate) rx: RefCell<Receiver<Bytes>>,
    pub(crate) entrypoint: Software<'a>,
    pub(crate) rt: Runtime,
    pub(crate) handle: RefCell<Option<JoinHandle<Result>>>,
    pub(crate) local: LocalSet,
}

impl Debug for Host<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Host")
            .field(&from_fn(|f| write!(f, "{}", &self.id)))
            .finish()
    }
}

impl<'a> Host<'a> {
    pub fn new<F, Fut>(bufsize: usize, software: F) -> Self
    where
        F: Fn() -> Fut + 'a,
        Fut: Future<Output = Result> + 'static,
    {
        let (tx, rx) = mpsc::channel::<Bytes>(bufsize);
        let id = MachineId::new();
        let software: Software = Box::new(move || Box::pin(software()));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap();

        Host {
            nic: MachineNic::new(tx, id),
            id,
            rx: rx.into(),
            entrypoint: software,
            rt,
            handle: None.into(),
            local: LocalSet::new(),
        }
    }
    // drops all received messages on the floor
    pub fn clear(&self) {
        while self.rx.borrow_mut().try_recv().is_ok() {}
    }

    pub fn poll_read(&self, cx: &mut Context<'_>) -> Poll<Option<bytes::Bytes>> {
        self.rx.borrow_mut().poll_recv(cx)
    }
    #[allow(clippy::await_holding_refcell_ref)]
    pub async fn read(&self) -> Option<Bytes> {
        self.rx.borrow_mut().recv().await
    }

    pub fn is_idle(&self) -> bool {
        self.handle.borrow().is_none()
    }

    // entrypoint here is a fn to allow for rebooting a host
    #[allow(clippy::async_yields_async)]
    pub fn start(&self) {
        // spawn_local immediately completes, adding the spawned task to
        // the LocalSet
        let local_spawn = || tokio::task::spawn_local((self.entrypoint)());
        let handle = self
            .rt
            // block_on enters a current thread runtime
            .block_on(async {
                // run until runs until local_spawn is run to completion
                self.local.run_until(async { local_spawn() }).await
            });
        // tldr; it queues entrypoint to run the next time someone calls
        // run_until. Notably run_until, when passed a Sleep future, will simply
        // fast forward the internal clock, running any tasks due in the process

        // stolen from turmoil :)
        // took me a sec to understand what it was doing
        *self.handle.borrow_mut() = Some(handle);
    }

    pub fn stop(&self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

impl Machine for Host<'_> {
    // This method is called by [`Sim::run`], which iterates through all the
    // runtimes and ticks each one. The magic of this method is described in the
    // documentation for [`LocalSet::run_until`], but it may not be entirely
    // obvious how things fit together.
    //
    // A [`LocalSet`] tracks the tasks to run, which may in turn spawn more
    // tasks. `run_until` drives a top level task to completion, but not its
    // children. If you look below, you may be confused. The task we run here
    // just sleeps and has no children! However, it's the _same `LocalSet`_ that
    // is used to run software on the host.
    //
    // In this way, every time `tick` is called, the following unfolds:
    //
    // 1. Time advances on the runtime
    // 2. We schedule a new task that simply sleeps
    // 3. Other tasks on the `LocalSet` get a chance to run
    // 4. The sleep finishes
    // 5. The runtime pauses
    //
    // Returns whether the software has finished successfully or the error
    // that caused failure. Subsequent calls do not return the error as it is
    // expected to fail the simulation.
    fn tick(&self, duration: Duration) -> std::result::Result<bool, Box<dyn std::error::Error>> {
        self.rt.block_on(async {
            self.local
                .run_until(async {
                    tokio::time::sleep(duration).await;
                })
                .await
        });

        // pull for software completion
        let mut borrowed = self.handle.borrow_mut();
        match &mut *borrowed {
            Some(handle) if handle.is_finished() => {
                // Consume handle to extract task result
                drop(borrowed);
                if let Some(h) = self.handle.take() {
                    match self.rt.block_on(h) {
                        // If the host was crashed the JoinError is cancelled, which needs to be
                        // handled to not fail the simulation.
                        Err(je) if je.is_cancelled() => {}
                        res => res??,
                    }
                };
                Ok(true)
            }
            Some(_) => Ok(false),
            None => Ok(true),
        }
    }

    fn id(&self) -> &MachineId {
        &self.id
    }
}

impl HasNic for Host<'_> {
    fn nic(&self) -> crate::sim::machine::MachineNic {
        self.nic.clone()
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
        assert!(h.tick(Duration::new(1, 0)).unwrap());
    }
}
