use std::{sync::Arc, time::Duration};

use dens::sim::RNG;
use quinn::{ConnectionId, ConnectionIdGenerator, Runtime as _};
use rand::Rng as _;
use tokio::time::sleep_until;
#[derive(Debug)]
pub struct TokioRuntime;

pub struct Timesource {
    start: std::time::Instant,
}

impl Timesource {
    pub fn new() -> Self {
        Self {
            start: TokioRuntime.now(),
        }
    }
}

impl quinn::TimeSource for Timesource {
    fn now(&self) -> std::time::SystemTime {
        let now = TokioRuntime.now();
        let diff = now.duration_since(self.start);
        std::time::SystemTime::UNIX_EPOCH + diff
    }
}

pub struct SeededCidGenerator;

impl ConnectionIdGenerator for SeededCidGenerator {
    fn generate_cid(&mut self) -> ConnectionId {
        RNG.with(|rng| {
            let mut rng = rng.borrow_mut();
            ConnectionId::new(&rng.random::<[u8; 20]>())
        })
    }

    fn cid_len(&self) -> usize {
        20
    }

    fn cid_lifetime(&self) -> Option<Duration> {
        None
    }
}

impl quinn::Runtime for TokioRuntime {
    fn new_timer(&self, i: std::time::Instant) -> std::pin::Pin<Box<dyn quinn::AsyncTimer>> {
        Box::pin(sleep_until(i.into()))
    }

    fn spawn(&self, future: std::pin::Pin<Box<dyn Future<Output = ()> + Send>>) {
        tokio::spawn(future);
    }

    fn wrap_udp_socket(
        &self,
        _t: std::net::UdpSocket,
    ) -> std::io::Result<Arc<dyn quinn::AsyncUdpSocket>> {
        unimplemented!()
    }

    fn now(&self) -> std::time::Instant {
        // panic if not in a tokio runtime
        let rt = tokio::runtime::Handle::current();
        let _g = rt.enter();

        tokio::time::Instant::now().into_std()
    }
}
