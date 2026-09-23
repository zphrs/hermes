use std::{
    backtrace::{Backtrace, BacktraceStatus},
    sync::Mutex,
};

/// Recorded when a [`request_loopback`](crate::cursor::requester::Requester::request_loopback)
/// future is dropped after its request was written but before its response
/// was read. Carries the backtrace of the cancellation site.
#[derive(Debug)]
pub struct Abandoned {
    backtrace: Backtrace,
}

impl std::fmt::Display for Abandoned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "a request_loopback call was cancelled after its write reached the wire but before the response was read"
        )?;
        match self.backtrace.status() {
            BacktraceStatus::Captured => write!(f, "\n{}", self.backtrace),
            _ => write!(
                f,
                "\nnote: run with `RUST_BACKTRACE=1` to see where the cancellation happened"
            ),
        }
    }
}

impl std::error::Error for Abandoned {}

/// Records an [`Abandoned`] into `slot` on drop unless defused.
pub(crate) struct LoopbackGuard<'a> {
    slot: &'a Mutex<Option<Abandoned>>,
    armed: bool,
}

impl<'a> LoopbackGuard<'a> {
    pub(crate) fn new(slot: &'a Mutex<Option<Abandoned>>) -> Self {
        Self { slot, armed: true }
    }

    pub(crate) fn defuse(mut self) {
        self.armed = false;
    }
}

impl Drop for LoopbackGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            *self.slot.lock().unwrap() = Some(Abandoned {
                backtrace: Backtrace::capture(),
            });
        }
    }
}
