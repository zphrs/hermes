mod checksum;
pub mod error;
pub mod ip;
pub mod udp;

use crate::{
    config::CONFIG,
    sim::{
        RNG,
        machine::{HasNic, Machine, MachineId, MachineNic},
    },
};
use bytes::Bytes;
use rand::Rng;
use std::{cell::RefCell, collections::HashMap, io::ErrorKind, ops::Deref, time::Duration};
use tokio::{runtime::Runtime, task::LocalSet};
use tracing::{trace, warn};

pub struct Network {
    id: MachineId,
    hosts: HashMap<MachineId, MachineNic>,
    rt: Runtime,
    local: LocalSet,
    unsent_messages: RefCell<Vec<(Bytes, Duration, MachineNic)>>,
}

impl Default for Network {
    fn default() -> Self {
        Self::new()
    }
}
impl Network {
    pub fn new() -> Self {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .unwrap();
        Network {
            id: MachineId::new(),
            hosts: Default::default(),
            rt,
            local: LocalSet::new(),
            unsent_messages: Default::default(),
        }
    }
    pub fn add_machine(&mut self, host: &dyn HasNic) {
        self.hosts.insert(host.id().clone(), host.nic());
    }

    pub fn add_machines<'a>(&mut self, hosts: impl IntoIterator<Item = &'a dyn HasNic>) {
        for host in hosts {
            self.add_machine(host);
        }
    }

    pub fn try_send_to_host(&self, id: &MachineId, posting: Bytes) -> std::io::Result<()> {
        let rand_dur = CONFIG.with(|cfg| {
            let latency = cfg.latency();
            RNG.with(|rng| latency.sample(rng.borrow_mut()))
        });
        let nic = self
            .hosts
            .get(&id)
            .ok_or(ErrorKind::HostUnreachable)?
            .clone();

        self.unsent_messages
            .borrow_mut()
            .push((posting, rand_dur, nic));
        Ok(())
    }
}

impl Machine for Network {
    fn tick(&self, duration: Duration) -> Result<bool, Box<dyn std::error::Error>> {
        for (posting, rand_dur, nic) in self.unsent_messages.borrow_mut().drain(..) {
            trace!("Adding msg to net");
            let message_loss = CONFIG.with(|cfg| cfg.message_loss_fail_rate());
            if RNG.with(|rng| rng.borrow_mut().random_bool(message_loss)) {
                warn!("dropped message: {:?}", posting);
                continue;
            }
            let _handle = self.rt.block_on(async {
                self.local
                    .run_until(async {
                        tokio::task::spawn_local(async move {
                            tokio::time::sleep(rand_dur).await;
                            nic.try_post(posting).await
                        })
                    })
                    .await
            });
        }
        self.rt.block_on(async {
            self.local
                .run_until(async {
                    tokio::time::sleep(duration).await;
                })
                .await
        });
        Ok(false)
    }

    fn id(&self) -> &crate::sim::machine::MachineId {
        &self.id
    }
}
