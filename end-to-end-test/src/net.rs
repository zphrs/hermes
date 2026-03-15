mod checksum;
pub mod error;
pub mod ip;
pub mod udp;

use crate::{
    config::CONFIG,
    sim::{
        RNG,
        machine::{BasicMachine, HasNic, Machine, MachineId, MachineNic},
    },
};
use bytes::Bytes;

use std::{collections::HashMap, io::ErrorKind, time::Duration};

#[derive(Default)]
struct NetworkInner {}

pub struct Network {
    machines: HashMap<MachineId, MachineNic>,
    inner_machine: BasicMachine,
}

impl Network {
    pub fn new() -> Self {
        let machine = CONFIG.with(|cfg| BasicMachine::new(cfg.ip_hop_capacity()));
        let out = Self {
            inner_machine: machine,
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
    pub fn add_machine(&mut self, host: &dyn HasNic) {
        self.machines.insert(host.id(), host.nic());
    }

    pub fn try_send_to_host(&self, id: &MachineId, posting: Bytes) -> std::io::Result<()> {
        let rand_dur = CONFIG.with(|cfg| {
            let latency = cfg.latency();
            RNG.with(|rng| latency.sample(rng.borrow_mut()))
        });
        let nic = self
            .machines
            .get(id)
            .ok_or(ErrorKind::HostUnreachable)?
            .clone();
        println!("waiting to send for {:?}", rand_dur);
        self.inner_machine.spawn_local(async move {
            tokio::time::sleep(rand_dur).await;
            let res = nic.try_post(posting).await;
            if let Err(e) = res {
                tracing::warn!("dropped message {}", e);
            }
            Ok(())
        });

        Ok(())
    }
}

impl Machine for Network {
    fn tick(&self, duration: Duration) -> Result<bool, Box<dyn std::error::Error>> {
        self.inner_machine.tick(duration)
    }

    fn id(&self) -> crate::sim::machine::MachineId {
        self.inner_machine.id()
    }

    fn is_idle(&self) -> bool {
        self.inner_machine.is_idle()
    }
}
