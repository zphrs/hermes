use tracing::Instrument as _;
mod checksum;
pub mod error;
pub mod ip;
pub mod udp;

use crate::sim::{
    RNG,
    config::CONFIG,
    machine::{BasicMachine, HasNic, Machine, MachineId, MachineNic},
};
use bytes::Bytes;
use rand::Rng;
use tracing::{info, trace, warn};

use std::rc::Rc;
use std::time::Duration;
use std::{collections::HashMap, io::ErrorKind};

pub struct Network {
    machines: HashMap<MachineId, (MachineNic, Duration)>,
    inner_machine: Rc<BasicMachine>,
}

impl Network {
    pub fn new() -> Self {
        let machine = CONFIG.with(|cfg| BasicMachine::new(cfg.ip_hop_capacity()));
        let out = Self {
            inner_machine: machine.into(),
            machines: Default::default(),
        };
        out
    }
}

impl Default for Network {
    fn default() -> Self {
        Self::new()
    }
}

impl Network {
    pub fn add_machine(&mut self, host: &impl HasNic) {
        let latency = CONFIG.with(|cfg| {
            let latency = cfg.latency();
            RNG.with(|rng| latency.sample(rng.borrow_mut()))
        });
        self.machines.insert(host.id(), (host.nic(), latency));
    }

    pub fn remove_machine(&mut self, host: &impl HasNic) {
        self.machines.remove(&host.id());
    }

    pub fn try_send_to_host(&self, id: &MachineId, posting: Bytes) -> std::io::Result<()> {
        let dropped = CONFIG.with(|cfg| {
            let dropped =
                RNG.with(|rng| rng.borrow_mut().random_bool(cfg.message_loss_fail_rate()));
            dropped
        });
        let (nic, latency) = self
            .machines
            .get(id)
            .ok_or(ErrorKind::HostUnreachable)?
            .clone();
        let jitter = RNG.with(|rng| {
            let stdev = latency.as_millis() as f64 / 5.0;
            let dist = rand_distr::Normal::new(0.0, stdev).unwrap();
            let jitter_ms = rng.borrow_mut().sample(dist) as i64;
            if jitter_ms < 0 {
                latency.saturating_sub(Duration::from_millis((-jitter_ms) as u64))
            } else {
                latency + Duration::from_millis(jitter_ms as u64)
            }
        });
        let span = tracing::debug_span!("send_to_host", to=?id);
        self.inner_machine.spawn_local(
            async move {
                if dropped {
                    trace!("dropped message due to chance");
                    return Ok(());
                }
                trace!("waiting to send for {:?}", jitter);
                tokio::time::sleep(jitter).await;
                let res = nic.try_post(posting).await;
                if let Err(e) = res {
                    tracing::warn!("failed to deliver message to nic: {}", e);
                }
                Ok(())
            }
            .instrument(span),
        );

        Ok(())
    }
}

impl Machine for Network {
    fn basic_machine(&self) -> Rc<BasicMachine> {
        self.inner_machine.clone()
    }

    fn is_idle(&self) -> bool {
        self.inner_machine.is_idle()
    }
}
