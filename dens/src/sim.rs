pub mod config;
pub mod machine;

use crate::{
    host::Result,
    os_mock::net::Dns,
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

type DefaultBuildHasher = BuildHasherDefault<DefaultHasher>;
type MachineTypesMap = HashMap<
    TypeId,
    HashMap<MachineId, Rc<dyn Any>, BuildHasherDefault<DefaultHasher>>,
    DefaultBuildHasher,
>;

/// responsible for default config, ticking all machines
pub struct Sim {
    machines: RefCell<MachineTypesMap>,
    basic_machines: RefCell<HashMap<MachineId, Rc<BasicMachine>, DefaultBuildHasher>>,
    config: Config,
    rng: RefCell<SmallRng>,
    dns: RefCell<Dns>,
}

impl Default for Sim {
    fn default() -> Self {
        Self {
            machines: RefCell::default(),
            basic_machines: RefCell::default(),
            config: Config::default(),
            rng: SmallRng::seed_from_u64(1234).into(),
            dns: RefCell::default(),
        }
    }
}

/// Can be used to get a reference to the machine via the [`get()`](Self::get()) function.
pub struct MachineRef<M: Machine + ?Sized> {
    id: MachineId,
    _phantom: PhantomData<M>,
}

pub trait MachineIntoRef: Machine {
    /// Adds this machine to the simulator and returns a [`MachineRef`] to it.
    ///
    /// It is necessary to add machines to the sim in order to have
    /// [`Sim::tick`] tick the machine. Equivalent to calling
    /// [`Sim::add_machine(machine)`](Sim::add_machine).
    ///
    /// # Panics
    ///
    /// Panics if it is called outside of a simulator runtime context.
    ///
    /// # Examples
    ///
    /// ```
    /// use dens::{Sim, BasicMachine, MachineIntoRef as _};
    /// # use std::rc::Rc;
    /// let sim = Sim::new();
    /// sim.enter_runtime(|| {
    /// let bm = Rc::new(BasicMachine::new(20)).into_ref();
    ///
    /// })
    /// ```
    #[must_use]
    fn into_ref(self) -> MachineRef<Self>
    where
        Self: Sized,
    {
        Sim::add_machine(self)
    }
}

impl<M: Machine> MachineIntoRef for M {}

impl<M: Machine> HasMachineId for MachineRef<M> {
    fn id(&self) -> MachineId {
        self.id
    }
}

impl<M: Machine> Clone for MachineRef<M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M: Machine> Copy for MachineRef<M> {}

impl<M: Machine> From<Rc<RefCell<M>>> for MachineRef<M> {
    fn from(value: Rc<RefCell<M>>) -> Self {
        MachineRef {
            id: value.borrow().id(),
            _phantom: PhantomData,
        }
    }
}

impl<M: Machine> MachineRef<M> {
    #[must_use]
    pub fn get(&self) -> Rc<RefCell<M>> {
        Sim::get_machine::<M>(self.id)
    }
}

impl<M: Machine> From<MachineId> for MachineRef<M> {
    fn from(value: MachineId) -> Self {
        MachineRef {
            id: value,
            _phantom: PhantomData,
        }
    }
}

impl Sim {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
    /// Creates a simulation with a custom [`Config`].
    ///
    /// # Examples
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
    #[must_use]
    pub fn new_with_config(config: Config) -> Self {
        Self {
            config,
            ..Default::default()
        }
    }
    /// # Panics
    ///
    /// Panics if it is called outside of a simulator runtime context.
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
    #[must_use]
    pub fn active() -> bool {
        SIM.is_set()
    }
    #[must_use]
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

    /// Returns [`None`](Option::None) if the machine does not exist, otherwise
    /// returns the raw machine that got removed.
    #[allow(clippy::must_use_candidate)]
    pub fn remove_machine<M: Machine>(machine_ref: MachineRef<M>) -> Option<M> {
        SIM.with(|v| {
            let id = machine_ref.id();
            let type_id = std::any::TypeId::of::<M>();
            let mut machines = v.machines.borrow_mut();
            let typed = machines.get_mut(&type_id)?;
            let out = typed.remove(&id);
            v.basic_machines.borrow_mut().remove(&id);
            #[expect(clippy::missing_panics_doc, reason = "infallible")]
            let machine: Rc<RefCell<M>> =
                Rc::downcast(out.expect("Machine with this id not found"))
                    .expect("types should match");
            Some(
                #[expect(clippy::missing_panics_doc, reason = "infallible")]
                Rc::try_unwrap(machine)
                    .map_err(|_| ())
                    .expect("unwrap should work")
                    .into_inner(),
            )
        })
    }
    /// # Panics
    ///
    /// Will panic if called outside of a sim runtime.
    #[must_use]
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

    /// # Panics
    ///
    /// Will panic if called outside of a machine instance / runtime.
    #[must_use]
    pub fn get_current_machine<M: Machine>() -> Rc<RefCell<M>> {
        let curr_machine = ACTIVE_MACHINE_ID.with(|id| *id);
        Self::get_machine(curr_machine)
    }

    /// # Panics
    ///
    /// Will panic if called outside of a machine instance / runtime.
    #[must_use]
    pub fn get_current_machine_ref<M: Machine>() -> MachineRef<M> {
        let curr_machine = ACTIVE_MACHINE_ID.with(|id| *id);
        MachineRef::from(curr_machine)
    }

    pub(crate) fn dns_mut(&self) -> impl DerefMut<Target = Dns> {
        self.dns.borrow_mut()
    }
    /// # Errors
    ///
    /// Will propagate up any errors returned from any of the machines.
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

    /// # Errors
    ///
    /// Will propagate up any errors returned from the machine being ticked.
    ///
    /// # Panics
    ///
    /// Will panic if the machine being referenced is not in this Sim.
    pub fn tick_machine<M: Machine>(m: MachineRef<M>) -> Result<bool> {
        SIM.with(|sim| {
            Self::on_machine(m, || {
                sim.basic_machines
                    .borrow()
                    .get(&m.id())
                    .unwrap()
                    .tick(CONFIG.with(Config::tick_amount))
            })
        })
    }

    /// # Errors
    ///
    /// Will propagate up any errors returned from the machine being ticked.
    ///
    /// # Panics
    ///
    /// Will panic if the machine being referenced is not in this Sim.
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
    ///
    /// # Errors
    ///
    /// Will propagate up any errors returned from any machine task.
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
                #[allow(clippy::cast_precision_loss)]
                let percent_idle = if total_count > 0 {
                    (count as f64 / total_count as f64) * 100.0
                } else {
                    0.0
                };
                let in_sim_elapsed = tick_count * CONFIG.with(Config::tick_amount);
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

    pub fn with_rng<F, R>(f: F) -> R
    where
        F: FnOnce(&mut SmallRng) -> R,
    {
        RNG.with(|rng| f(&mut rng.borrow_mut()))
    }

    pub(crate) fn on_machine<R>(server: MachineRef<impl Machine>, f: impl FnOnce() -> R) -> R {
        ACTIVE_MACHINE_ID.set(&server.id, f)
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use crate::{
        IpNetwork, OsMock, Sim,
        os_mock::net::UdpSocket,
        sim::{Config, MachineIntoRef as _},
    };

    #[test_log::test]
    pub fn dns_example() {
        let sim = Sim::new_with_config(Config::synchronous_network());

        sim.enter_runtime(|| {
            sim.dns_mut().insert("example.com", [10, 0, 0, 1]);
            sim.dns_mut().insert(
                "example.com",
                Ipv4Addr::from([10, 0, 0, 1]).to_ipv6_mapped(),
            );
            let net = IpNetwork::new_private_class_c().into_ref();
            let server = OsMock::new(|| async {
                let sock = UdpSocket::bind("example.com:80").await?;
                let mut buf = [0u8; b"hello".len()];
                sock.recv(&mut buf).await?;
                assert_eq!(b"hello", &buf);
                Ok(())
            });
            server.connect_to_net_with_ipv4(net, [10, 0, 0, 1]);
            let server = server.into_ref();
            let client = OsMock::new(|| async {
                let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
                sock.connect("example.com:80").await?;
                sock.send(b"hello").await?;
                Ok(())
            });
            client.connect_to_net(net);
            let client = client.into_ref();
            let arr = [client, server];
            Sim::run_until_idle(|| arr.iter()).unwrap();
        });
    }
}
