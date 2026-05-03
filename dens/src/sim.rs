pub mod config;
pub mod machine;

use crate::{
    host::{Result, net::dns::Dns},
    sim::{
        config::CONFIG,
        machine::{BasicMachine, HasMachineId, Machine, MachineId},
    },
};
pub use config::Config;

use rand::{SeedableRng as _, rngs::SmallRng};
use scoped_tls::scoped_thread_local;
use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::HashMap,
    hash::{BuildHasherDefault, DefaultHasher},
    marker::PhantomData,
    ops::{DerefMut, Div},
    rc::Rc,
    time::Duration,
};

scoped_thread_local!(pub static RNG: RefCell<SmallRng>);
scoped_thread_local!(pub(crate) static ACTIVE_MACHINE_ID: MachineId);
scoped_thread_local!(pub static SIM: Sim);

/// responsible for default config, ticking all machines
pub struct Sim {
    machines: RefCell<
        HashMap<
            TypeId,
            HashMap<MachineId, Rc<dyn Any>, BuildHasherDefault<DefaultHasher>>,
            BuildHasherDefault<DefaultHasher>,
        >,
    >,
    basic_machines:
        RefCell<HashMap<MachineId, Rc<BasicMachine>, BuildHasherDefault<DefaultHasher>>>,
    config: Config,
    rng: RefCell<SmallRng>,
    dns: RefCell<Dns>,
}

impl Default for Sim {
    fn default() -> Self {
        Self {
            machines: Default::default(),
            basic_machines: Default::default(),
            config: Default::default(),
            rng: SmallRng::seed_from_u64(1234).into(),
            dns: Default::default(),
        }
    }
}

pub struct MachineRef<M: Machine + ?Sized> {
    id: MachineId,
    _phantom: PhantomData<M>,
}

impl<M: Machine> HasMachineId for MachineRef<M> {
    fn id(&self) -> MachineId {
        self.id
    }
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
            id: value.borrow().id(),
            _phantom: Default::default(),
        }
    }
}

impl<M: Machine> MachineRef<M> {
    pub fn get(&self) -> Rc<RefCell<M>> {
        Sim::get_machine::<M>(self.id)
    }
}

impl<M: Machine> From<MachineId> for MachineRef<M> {
    fn from(value: MachineId) -> Self {
        MachineRef {
            id: value,
            _phantom: Default::default(),
        }
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
    /// Example: Create a simulation with a custom config
    ///
    /// ```
    /// # use dens::sim::Sim;
    /// # use std::time::Duration;
    /// # use dens::sim::Config;
    /// let sim = Sim::new_with_config(Config {
    ///     tick_amount: Duration::from_millis(100),
    ///     ..Default::default()
    /// });
    /// ```
    pub fn new_with_config(config: Config) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }

    pub fn add_machine<M: Machine>(machine: M) -> MachineRef<M> {
        SIM.with(|v| {
            let id = machine.id();
            let basic_machine = machine.basic_machine();
            let mut binding = v.machines.borrow_mut();
            let typed = binding.entry(TypeId::of::<M>()).or_default();
            let out = Rc::new(RefCell::new(machine));
            typed.insert(id, out.clone());
            v.basic_machines.borrow_mut().insert(id, basic_machine);
            out.into()
        })
    }

    pub fn active() -> bool {
        SIM.is_set()
    }

    pub fn is_in_machine_type<M: Machine>() -> bool {
        SIM.with(|v| {
            let type_id = std::any::TypeId::of::<M>();
            let machines = v.machines.borrow();
            let Some(of_type) = machines.get(&type_id) else {
                return false;
            };
            ACTIVE_MACHINE_ID.with(|id| of_type.contains_key(id))
        })
    }

    pub fn remove_machine<M: Machine>(machine_ref: MachineRef<M>) -> M {
        SIM.with(|v| {
            let id = machine_ref.id();
            let type_id = std::any::TypeId::of::<M>();
            let mut machines = v.machines.borrow_mut();
            let typed = machines
                .get_mut(&type_id)
                .expect("No machines of this type registered");
            let out = typed.remove(&id);
            v.basic_machines.borrow_mut().remove(&id);
            let machine: Rc<RefCell<M>> =
                Rc::downcast(out.expect("Machine with this id not found"))
                    .expect("types should match");
            Rc::try_unwrap(machine)
                .map_err(|_| ())
                .expect("unwrap should work")
                .into_inner()
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

    /// will panic if called outside of a machine instance / runtime
    pub fn get_current_machine_ref<M: Machine>() -> MachineRef<M> {
        let curr_machine = ACTIVE_MACHINE_ID.with(|id| *id);
        MachineRef::from(curr_machine)
    }

    pub(crate) fn dns_mut(&self) -> impl DerefMut<Target = Dns> {
        self.dns.borrow_mut()
    }

    pub fn tick() -> Result<bool> {
        SIM.with(|sim| {
            let mut is_done = true;
            for (id, m) in &*sim.basic_machines.borrow() {
                if m.is_idle() {
                    continue;
                }
                is_done =
                    ACTIVE_MACHINE_ID.set(id, || m.tick(sim.config.tick_amount()))? && is_done;
            }
            Ok(is_done)
        })
    }

    pub fn tick_machine<M: Machine>(m: MachineRef<M>) -> Result<bool> {
        SIM.with(|sim| {
            Self::on_machine(m, || {
                sim.basic_machines
                    .borrow()
                    .get(&m.id())
                    .unwrap()
                    .tick(CONFIG.with(|cfg| cfg.tick_amount()))
            })
        })
    }

    pub fn tick_machine_with_duration<M: Machine>(
        duration: Duration,
        m: MachineRef<M>,
    ) -> Result<bool> {
        SIM.with(|sim| {
            Self::on_machine(m, || {
                sim.basic_machines
                    .borrow()
                    .get(&m.id())
                    .unwrap()
                    .tick(duration)
            })
        })
    }

    pub fn enter_runtime<R>(&self, f: impl FnOnce() -> R) -> R {
        SIM.set(self, || {
            CONFIG.set(&self.config, || RNG.set(&self.rng.clone(), f))
        })
    }

    /// The passed in iterator just determines which machines' [`Machine::is_idle()`]
    /// status is checked in order to stop ticking.
    /// The sim will run all machines in lockstep.
    ///
    /// If you want to simulate machines with clock skew,
    /// call [`Sim::tick_machine()`] or [`Sim::tick_machine_with_duration()`].
    /// Checking if it should stop and logging diagnostics is done every 100 ticks.
    pub fn run_until_idle<'a, M: Machine, I: ExactSizeIterator<Item = &'a MachineRef<M>>>(
        f: impl Fn() -> I,
    ) -> Result {
        let mut tick_count = 0;
        let mut last_log_time = std::time::Instant::now();

        while !Sim::tick()? {
            tick_count += 1;
            tracing::debug!(tick_count);
            if tick_count % 100 == 0 {
                let elapsed = last_log_time.elapsed();
                let iter = f();
                let total_count = iter.len();
                let count = iter
                    .filter(|machine: &&MachineRef<M>| machine.get().borrow().is_idle())
                    .count();
                let percent_idle = if total_count > 0 {
                    (count as f64 / total_count as f64) * 100.0
                } else {
                    0.0
                };
                let in_sim_elapsed = tick_count * CONFIG.with(|cfg| cfg.tick_amount());
                tracing::info!(
                    "ticking {} ({:?}), {:.1}% idle, ({} machines left) in_sim_elapsed: {:?}",
                    tick_count,
                    elapsed.div(100),
                    percent_idle,
                    total_count - count,
                    in_sim_elapsed
                );
                last_log_time = std::time::Instant::now();

                if count == total_count {
                    break;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn on_machine<R>(server: MachineRef<impl Machine>, f: impl FnOnce() -> R) -> R {
        ACTIVE_MACHINE_ID.set(&server.id, f)
    }
}
