use std::{ops::DerefMut, time::Duration};

use rand::Rng;
use rand_distr::{Distribution as _, Exp};
use scoped_tls::scoped_thread_local;

scoped_thread_local!(pub(crate) static CONFIG: Config);

/// Configure how often messages are lost.
///
/// Provides default values of 1% chance of a message being dropped
#[derive(Clone, Copy)]
pub(crate) struct MessageLoss {
    /// Probability of a message being dropped/corrupted
    pub(crate) fail_rate: f64,
}

impl Default for MessageLoss {
    fn default() -> Self {
        Self { fail_rate: 0.01 }
    }
}

/// Configure latency behavior between two hosts.
///
/// Provides default values of:
/// - min_message_latency: 0ms
/// - max_message_latency: 100ms
/// - latency_distribution: Exp(5)
#[derive(Clone, Copy)]
pub(crate) struct Latency {
    /// Minimum latency
    min_message_latency: Duration,

    /// Maximum latency
    max_message_latency: Duration,

    /// Probability distribution of latency within the range above.
    latency_distribution: Exp<f64>,
}

impl Latency {
    pub fn sample<R>(&self, mut rand: impl DerefMut<Target = R>) -> Duration
    where
        R: Rng + ?Sized,
    {
        let mult = self.latency_distribution.sample(rand.deref_mut());
        let range = (self.max_message_latency - self.min_message_latency).as_millis() as f64;
        self.min_message_latency + Duration::from_millis((range * mult) as _)
    }
}

impl Default for Latency {
    fn default() -> Self {
        Self {
            min_message_latency: Duration::from_millis(50),
            max_message_latency: Duration::from_millis(500),
            latency_distribution: Exp::new(5.0).unwrap(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Config {
    udp_capacity: usize,
    tick_amount: Duration,
    latency: Latency,
    message_loss: MessageLoss,
}
impl Config {
    pub fn latency(&self) -> &Latency {
        &self.latency
    }
    pub fn udp_capacity(&self) -> usize {
        self.udp_capacity
    }
    pub fn tick_amount(&self) -> Duration {
        self.tick_amount
    }

    pub fn message_loss_fail_rate(&self) -> f64 {
        self.message_loss.fail_rate
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // can only buffer 100 messages at a time, otherwise drops msg on floor
            udp_capacity: 100,
            // granularity of tick(), necessary to tick to simulate clock skew between
            // hosts
            tick_amount: Duration::from_millis(1),
            latency: Default::default(),
            message_loss: Default::default(),
        }
    }
}
