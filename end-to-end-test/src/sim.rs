pub mod machine;

use crate::{
    config::{CONFIG, Config},
    host::{Result, net::dns::Dns},
    sim::machine::{Machine, MachineId},
};

use rand::{SeedableRng as _, rngs::SmallRng};
use scoped_tls::scoped_thread_local;
use std::{
    any::TypeId,
    cell::RefCell,
    collections::HashMap,
    hash::{BuildHasherDefault, DefaultHasher},
    marker::PhantomData,
    ops::DerefMut,
    rc::Rc,
};

scoped_thread_local!(pub(crate) static RNG: RefCell<SmallRng>);
scoped_thread_local!(pub(crate) static ACTIVE_MACHINE_ID: MachineId);
scoped_thread_local!(pub static SIM: Sim);

/// responsible for default config, ticking all machines
pub struct Sim {
    machines: RefCell<
        HashMap<
            TypeId,
            HashMap<MachineId, Rc<dyn Machine>, BuildHasherDefault<DefaultHasher>>,
            BuildHasherDefault<DefaultHasher>,
        >,
    >,
    config: Config,
    rng: RefCell<SmallRng>,
    dns: RefCell<Dns>,
}

impl Default for Sim {
    fn default() -> Self {
        Self {
            machines: Default::default(),
            config: Default::default(),
            rng: SmallRng::seed_from_u64(1234).into(),
            dns: Default::default(),
        }
    }
}

pub struct MachineRef<M: Machine> {
    id: MachineId,
    _phantom: PhantomData<M>,
}

impl<M: Machine> Clone for MachineRef<M> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            _phantom: self._phantom.clone(),
        }
    }
}

impl<M: Machine> Copy for MachineRef<M> {}

impl<M: Machine> From<Rc<RefCell<M>>> for MachineRef<M> {
    fn from(value: Rc<RefCell<M>>) -> Self {
        MachineRef {
            id: value.id(),
            _phantom: Default::default(),
        }
    }
}

impl<M: Machine> MachineRef<M> {
    pub fn get(&self) -> Rc<RefCell<M>> {
        Sim::get_machine::<M>(self.id)
    }
}

impl<M: Machine> Machine for MachineRef<M> {
    fn tick(&self, duration: std::time::Duration) -> Result<bool> {
        self.get().borrow().tick(duration)
    }

    fn id(&self) -> MachineId {
        self.id
    }

    fn is_idle(&self) -> bool {
        self.get().borrow().is_idle()
    }
}

impl Sim {
    /// Register a host with the simulation.
    ///
    /// This method takes a `Fn` that builds a future, as opposed to
    /// [`Sim::client`] which just takes a future. The reason for this is we
    /// might restart the host, and so need to be able to call the future
    /// multiple times.
    pub fn new() -> Self {
        Default::default()
    }

    pub fn add_machine<M: Machine>(machine: M) -> MachineRef<M> {
        SIM.with(|v| {
            let id = machine.id();
            let mut binding = v.machines.borrow_mut();
            let typed = binding.entry(TypeId::of::<M>()).or_default();
            let out = Rc::new(RefCell::new(machine));
            typed.insert(id, out.clone());
            out.into()
        })
    }
    /// will panic if called outside of a sim runtime
    pub fn get_machine<M: Machine + 'static>(id: MachineId) -> Rc<RefCell<M>> {
        let type_id = std::any::TypeId::of::<M>();
        SIM.with(|sim| {
            let machines = sim.machines.borrow();
            let typed = machines
                .get(&type_id)
                .expect("No machines of this type registered");
            let machine = typed
                .get(&id)
                .expect("Machine with this id not found")
                .clone();

            let other: Rc<RefCell<M>> = Rc::downcast(machine).expect("types should match");
            other
        })
    }
    /// will panic if called outside of a machine instance / runtime
    pub fn get_current_machine<M: Machine>() -> Rc<RefCell<M>> {
        let curr_machine = ACTIVE_MACHINE_ID.with(|id| *id);
        Self::get_machine(curr_machine)
    }

    pub(crate) fn dns_mut(&self) -> impl DerefMut<Target = Dns> {
        self.dns.borrow_mut()
    }

    pub fn tick() -> Result<bool> {
        SIM.with(|sim| {
            let mut is_done = true;
            for (_type, map) in &*sim.machines.borrow() {
                for (id, m) in map {
                    is_done =
                        ACTIVE_MACHINE_ID.set(id, || m.tick(sim.config.tick_amount()))? && is_done;
                }
            }
            Ok(is_done)
        })
    }

    pub fn tick_machine<M: Machine>(m: MachineRef<M>) -> Result<bool> {
        Self::on_machine(m, || m.get().tick(CONFIG.with(|cfg| cfg.tick_amount())))
    }

    pub fn enter_runtime<R>(&self, f: impl FnOnce() -> R) -> R {
        SIM.set(self, || {
            CONFIG.set(&self.config, || RNG.set(&self.rng.clone(), f))
        })
    }

    pub fn run_until_idle() -> Result {
        while !Sim::tick()? {}
        Ok(())
    }

    pub(crate) fn on_machine<R>(server: MachineRef<impl Machine>, f: impl FnOnce() -> R) -> R {
        ACTIVE_MACHINE_ID.set(&server.id, f)
    }
}
