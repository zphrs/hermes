pub mod machine;

use crate::{
    config::{CONFIG, Config},
    host::{Result, net::dns::Dns},
    sim::machine::{Machine, MachineId},
};

use rand::{SeedableRng as _, rngs::SmallRng};
use scoped_tls::scoped_thread_local;
use std::{
    cell::RefCell,
    collections::HashMap,
    hash::{BuildHasherDefault, DefaultHasher},
    ops::DerefMut,
    rc::Rc,
};

scoped_thread_local!(pub(crate) static RNG: RefCell<SmallRng>);
scoped_thread_local!(pub(crate) static ACTIVE_MACHINE_ID: MachineId);
scoped_thread_local!(pub(crate) static SIM: Sim);

/// responsible for default config, ticking all machines
pub struct Sim<'a> {
    rts: HashMap<MachineId, Rc<RefCell<dyn Machine + 'a>>, BuildHasherDefault<DefaultHasher>>,
    config: Config,
    rng: RefCell<SmallRng>,
    dns: RefCell<Dns>,
}

impl<'a> Default for Sim<'a> {
    fn default() -> Self {
        Self {
            rts: Default::default(),
            config: Default::default(),
            rng: SmallRng::seed_from_u64(1234).into(),
            dns: Default::default(),
        }
    }
}

impl<'a> Sim<'a> {
    /// Register a host with the simulation.
    ///
    /// This method takes a `Fn` that builds a future, as opposed to
    /// [`Sim::client`] which just takes a future. The reason for this is we
    /// might restart the host, and so need to be able to call the future
    /// multiple times.
    pub fn new() -> Self {
        Default::default()
    }
    pub fn add_machine<M: Machine + 'a>(&mut self, machine: M) -> Rc<RefCell<M>> {
        let id = *machine.id();
        let wrapped = Rc::new(RefCell::new(machine));
        self.rts.insert(id, wrapped.clone());
        wrapped
    }

    pub(crate) fn dns_mut(&self) -> impl DerefMut<Target = Dns> {
        self.dns.borrow_mut()
    }

    pub fn tick(&self) -> Result {
        self.enter_runtime(|| {
            for (id, host) in &self.rts {
                ACTIVE_MACHINE_ID.set(id, || {
                    RefCell::<dyn Machine>::borrow(host).tick(self.config.tick_amount())
                })?;
            }
            Ok(())
        })
    }

    pub fn enter_runtime<R>(&self, f: impl FnOnce() -> R) -> R {
        CONFIG.set(&self.config, || RNG.set(&self.rng.clone(), f))
    }

    pub fn on_machine<R>(&self, machine: &dyn Machine, f: impl FnOnce() -> R) -> R {
        self.enter_runtime(|| ACTIVE_MACHINE_ID.set(machine.id(), f))
    }
}
