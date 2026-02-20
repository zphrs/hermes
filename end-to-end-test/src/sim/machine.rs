use std::{
    any::Any,
    fmt::{Debug, Display},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use bytes::Bytes;
use tokio::sync::mpsc::{self, Sender};
use tracing::instrument;

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

pub trait Machine {
    /// Returns whether the machine has finished all its tasks or the error
    /// that caused the failure.  Subsequent calls do not return the error as it
    /// is expected to fail the simulation.
    fn tick(&self, duration: Duration) -> Result<bool, Box<dyn std::error::Error>>;

    fn id(&self) -> &MachineId;
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
