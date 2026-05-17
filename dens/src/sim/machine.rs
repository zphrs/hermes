use std::{
    any::Any,
    cell::RefCell,
    fmt::{Debug, Display},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use tokio::{
    runtime::Runtime,
    sync::mpsc::{self, Receiver, Sender},
    task::{AbortHandle, JoinSet, LocalSet},
    time::Instant,
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
        f.debug_tuple("HostNic").field(&self.parent_id).finish()
    }
}

impl MachineNic {
    #[must_use]
    pub fn new(tx: Sender<Bytes>, parent_id: MachineId) -> Self {
        Self { tx, parent_id }
    }

    /// Drops message on floor if recipient's buffer is full
    /// Logs error from [`Self::try_post`] as a warning
    #[instrument(skip(posting))]
    pub fn post(&self, posting: Bytes) {
        if let Err(e) = self.try_post(posting) {
            tracing::warn!("dropping posted packet because {}", e);
        }
    }

    /// # Errors
    ///
    /// Returns [`std::io::ErrorKind::QuotaExceeded`] if the recipient's buffer is full,
    /// or [`std::io::ErrorKind::HostUnreachable`] if the recipient has been dropped.
    #[instrument(skip(posting))]
    pub fn try_post(&self, posting: Bytes) -> std::io::Result<()> {
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

pub trait HasNic: HasMachineId {
    fn nic(&self) -> MachineNic;
}

pub trait HasMachineId {
    fn id(&self) -> MachineId;
}

impl HasMachineId for MachineId {
    fn id(&self) -> MachineId {
        *self
    }
}

impl<M: Machine> HasMachineId for M {
    fn id(&self) -> MachineId {
        BasicMachine::id(&self.basic_machine())
    }
}

pub trait Machine: Any + HasMachineId {
    /// Returns whether the machine has finished all its tasks or the error that
    /// caused the failure. Subsequent calls do not return the error as it is
    /// expected to fail the simulation.
    fn basic_machine(&self) -> Rc<BasicMachine>;

    fn is_idle(&self) -> bool;
}

pub struct BasicMachine {
    id: MachineId,
    nic: MachineNic,
    rx: RefCell<Receiver<Bytes>>,
    rt: Runtime,
    local: LocalSet,
    js: RefCell<JoinSet<Result>>,
    curr_byte_buf: Bytes,
    start_time: tokio::time::Instant,
}

impl BasicMachine {
    #[must_use]
    pub fn new(bufsize: usize) -> BasicMachine {
        let (tx, rx) = mpsc::channel::<Bytes>(bufsize);
        let id = MachineId::new();
        #[expect(clippy::missing_panics_doc, reason = "infallible")]
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap();

        // enter runtime here to ensure that the start_time is the start_time of
        // the machine
        let _guard = rt.enter();
        #[allow(clippy::default_trait_access)]
        Self {
            nic: MachineNic::new(tx, id),
            id,
            rx: rx.into(),
            rt,
            local: LocalSet::new(),
            js: Default::default(),
            curr_byte_buf: Default::default(),
            start_time: Instant::now(),
        }
    }

    pub fn sys_time(&self) -> std::time::SystemTime {
        use std::time::SystemTime;
        #[expect(
            clippy::missing_panics_doc,
            reason = "infallible unless the system is up for 2^32 seconds"
        )]
        SystemTime::UNIX_EPOCH
            .checked_add(self.start_time.elapsed())
            .expect("system shouldn't be alive for enough time (2^32 seconds) to cause an overflow")
    }

    pub fn poll_read_bytes(&self, cx: &mut Context<'_>) -> Poll<Option<bytes::Bytes>> {
        self.rx.borrow_mut().poll_recv(cx)
    }

    pub async fn read(&self) -> Option<bytes::Bytes> {
        self.rx.borrow_mut().recv().await
    }
    /// Spawns the [`task`] on the machine. Key way to run software.
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

    /// # Errors
    ///
    /// Will propagate up any errors thrown during the execution of the
    /// test.
    pub fn tick(&self, duration: Duration) -> Result<bool> {
        self.rt.block_on(async {
            self.local
                .run_until(async {
                    tokio::time::sleep(duration).await;
                })
                .await;
        });
        // throw any error up the chain
        while let Some(next) = self.js.borrow_mut().try_join_next() {
            next??;
        }

        Ok(self.js.borrow().is_empty())
    }

    pub fn is_idle(&self) -> bool {
        self.js.borrow().is_empty()
    }
}

impl HasMachineId for BasicMachine {
    fn id(&self) -> MachineId {
        self.id
    }
}

impl HasNic for BasicMachine {
    fn nic(&self) -> MachineNic {
        self.nic.clone()
    }
}

impl Machine for Rc<BasicMachine> {
    fn basic_machine(&self) -> Rc<BasicMachine> {
        self.clone()
    }

    fn is_idle(&self) -> bool {
        BasicMachine::is_idle(self)
    }
}

impl futures_io::AsyncRead for BasicMachine {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.curr_byte_buf.is_empty() {
            match self.poll_read_bytes(cx) {
                Poll::Ready(Some(bytes)) => {
                    let out_len = std::cmp::min(bytes.len(), buf.len());
                    buf.copy_from_slice(&bytes[..out_len]);
                    self.curr_byte_buf = bytes.slice(out_len..);
                    Poll::Ready(Ok(out_len))
                }
                Poll::Ready(None) => Poll::Ready(Ok(0)),
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
        write!(f, "{:04x}", self.id)
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
