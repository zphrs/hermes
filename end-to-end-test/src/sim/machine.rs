use std::{
    any::Any,
    cell::RefCell,
    fmt::{Debug, Display},
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use tokio::{
    runtime::Runtime,
    sync::mpsc::{self, Receiver, Sender},
    task::{AbortHandle, JoinSet, LocalSet},
};
use tracing::instrument;

use crate::host::Result;

#[derive(Clone)]
pub struct MachineNic {
    pub(crate) tx: Sender<Bytes>,
    pub(crate) parent_id: MachineId,
}

impl Debug for MachineNic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostNic")
            .field("parent_id", &self.parent_id)
            .finish()
    }
}

impl MachineNic {
    pub fn new(tx: Sender<Bytes>, parent_id: MachineId) -> Self {
        Self { tx, parent_id }
    }

    /// Drops message on floor if recipient's buffer is full
    /// Logs error as a warning
    #[instrument(skip(posting))]
    pub async fn post(&self, posting: Bytes) {
        if let Err(e) = self.try_post(posting).await {
            tracing::warn!("dropping posted packet because {}", e)
        }
    }

    #[instrument(skip(posting))]
    // will drop message on floor if recipient's buffer is full
    pub async fn try_post(&self, posting: Bytes) -> std::io::Result<()> {
        self.tx.try_send(posting).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => {
                std::io::Error::new(std::io::ErrorKind::QuotaExceeded, e)
            }
            mpsc::error::TrySendError::Closed(_) => {
                std::io::Error::new(std::io::ErrorKind::HostUnreachable, e)
            }
        })?;
        Ok(())
    }
}

pub trait HasNic: Machine {
    fn nic(&self) -> MachineNic;
}

pub trait Machine: Any {
    /// Returns whether the machine has finished all its tasks or the error
    /// that caused the failure.  Subsequent calls do not return the error as it
    /// is expected to fail the simulation.
    fn tick(&self, duration: Duration) -> Result<bool>;

    fn id(&self) -> MachineId;

    fn is_idle(&self) -> bool;
}

impl<T: Machine> Machine for RefCell<T> {
    fn tick(&self, duration: Duration) -> Result<bool> {
        self.borrow().tick(duration)
    }

    fn id(&self) -> MachineId {
        self.borrow().id()
    }

    fn is_idle(&self) -> bool {
        self.borrow().is_idle()
    }
}

pub struct BasicMachine {
    id: MachineId,
    nic: MachineNic,
    rx: RefCell<Receiver<Bytes>>,
    rt: Runtime,
    local: LocalSet,
    js: RefCell<JoinSet<Result>>,
    curr_byte_buf: Bytes,
}

impl BasicMachine {
    pub fn new(bufsize: usize) -> BasicMachine {
        let (tx, rx) = mpsc::channel::<Bytes>(bufsize);
        let id = MachineId::new();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap();
        Self {
            nic: MachineNic::new(tx, id),
            id,
            rx: rx.into(),
            rt,
            local: LocalSet::new(),
            js: Default::default(),
            curr_byte_buf: Default::default(),
        }
    }

    pub fn poll_read_bytes(&self, cx: &mut Context<'_>) -> Poll<Option<bytes::Bytes>> {
        self.rx.borrow_mut().poll_recv(cx)
    }

    #[allow(clippy::await_holding_refcell_ref)]
    pub async fn read(&self) -> Option<Bytes> {
        self.rx.borrow_mut().recv().await
    }

    pub fn spawn_local<Fut>(&self, task: Fut) -> AbortHandle
    where
        Fut: Future<Output = Result> + 'static,
    {
        // spawn_local immediately completes, adding the spawned task to
        // the JoinSet
        self.js.borrow_mut().spawn_local_on(task, &self.local)
    }

    /// drops all received messages on the floor
    pub fn clear_messages(&self) {
        while self.rx.borrow_mut().try_recv().is_ok() {}
    }
    /// Aborts all spawned tasks
    pub fn abort_all(&self) {
        self.js.borrow_mut().abort_all();
    }

    pub fn tasks_left(&self) -> usize {
        self.js.borrow().len()
    }
}

impl HasNic for BasicMachine {
    fn nic(&self) -> MachineNic {
        self.nic.clone()
    }
}

impl Machine for BasicMachine {
    // This method is called by [`Sim::run`], which iterates through all the
    // runtimes and ticks each one. The magic of this method is described in the
    // documentation for [`LocalSet::run_until`], but it may not be entirely
    // obvious how things fit together.
    //
    // A [`LocalSet`] tracks the tasks to run, which may in turn spawn more
    // tasks. `run_until` drives a top level task to completion, but not its
    // children. If you look below, you may be confused. The task we run here
    // just sleeps and has no children! However, it's the _same `JoinSet`_ that
    // is used to run software on the host.
    //
    // In this way, every time `tick` is called, the following unfolds:
    //
    // 1. We schedule a new task that simply sleeps
    // 2. Time advances on the runtime
    // 3. Other tasks on the `LocalSet` get a chance to run
    // 4. The sleep finishes
    // 5. The runtime pauses
    //
    // Returns whether the software has finished successfully or the error
    // that caused failure. Subsequent calls do not return the error as it is
    // expected to fail the simulation.
    fn tick(&self, duration: Duration) -> Result<bool> {
        self.rt.block_on(async {
            self.local
                .run_until(async {
                    tokio::time::sleep(duration).await;
                })
                .await
        });
        // throw any error up the chain
        while let Some(next) = self.js.borrow_mut().try_join_next() {
            next??;
        }

        Ok(self.js.borrow().is_empty())
    }

    fn id(&self) -> MachineId {
        self.id
    }

    fn is_idle(&self) -> bool {
        self.js.borrow().is_empty()
    }
}

impl futures_io::AsyncRead for BasicMachine {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.curr_byte_buf.len() == 0 {
            match self.poll_read_bytes(cx) {
                Poll::Ready(Some(bytes)) => {
                    let out_len = std::cmp::min(bytes.len(), buf.len());
                    buf.copy_from_slice(&bytes[..out_len]);
                    self.curr_byte_buf = bytes.slice(out_len..);
                    Poll::Ready(Ok(out_len))
                }
                Poll::Ready(None) => return Poll::Ready(Ok(0)),
                Poll::Pending => Poll::Pending,
            }
        } else {
            let out_len = std::cmp::min(self.curr_byte_buf.len(), buf.len());
            buf.copy_from_slice(&self.curr_byte_buf[..out_len]);
            Poll::Ready(Ok(out_len))
        }
    }
}

/// Intentionally opaque type only for uniquely identifying hosts.
/// There is never a reason to have a mutable pointer to a HostId.
#[derive(Hash, PartialEq, Eq, Clone, Copy)]
pub struct MachineId {
    pub(crate) id: u64,
}

impl Default for MachineId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for MachineId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:#x}", self.id)
    }
}

impl Debug for MachineId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self}")
    }
}

impl MachineId {
    pub fn new() -> Self {
        pub(crate) static CTR: AtomicU64 = AtomicU64::new(0);
        MachineId {
            id: CTR.fetch_add(1, Ordering::AcqRel),
        }
    }
}
