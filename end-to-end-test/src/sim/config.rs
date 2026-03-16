use std::{ops::DerefMut, time::Duration};

use rand::Rng;
use rand_distr::{Distribution as _, Exp};
use scoped_tls::scoped_thread_local;

scoped_thread_local!(pub(crate) static CONFIG: Config);

/// Configure how often messages are lost.
///
/// Provides default values of 1% chance of a message being dropped
#[derive(Clone, Copy)]
pub struct MessageLoss {
    /// Probability of a message being dropped/corrupted
    pub fail_rate: f64,
}

impl Default for MessageLoss {
    fn default() -> Self {
        Self { fail_rate: 0.0 }
    }
}

/// Configure latency behavior between two hosts.
///
/// Provides default values of:
/// - min_message_latency: 0ms
/// - max_message_latency: 100ms
/// - latency_distribution: Exp(5)
#[derive(Clone, Copy)]
pub struct Latency {
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
        let sample = self.latency_distribution.sample(rand.deref_mut());
        let latency_ms = self.min_message_latency.as_millis() as f64 + sample;
        let clamped = latency_ms.clamp(
            self.min_message_latency.as_millis() as f64,
            self.max_message_latency.as_millis() as f64,
        );
        Duration::from_millis(clamped as u64)
    }
}

impl Default for Latency {
    fn default() -> Self {
        Self {
            min_message_latency: Duration::from_millis(50),
            max_message_latency: Duration::from_millis(500),
            latency_distribution: Exp::new(10.0).unwrap(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Config {
    pub udp_capacity: usize,
    pub ip_hop_capacity: usize,
    pub nic_capacity: usize,
    pub tick_amount: Duration,
    pub latency: Latency,
    pub message_loss: MessageLoss,
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
    pub fn ip_hop_capacity(&self) -> usize {
        self.ip_hop_capacity
    }

    pub fn nic_capacity(&self) -> usize {
        self.nic_capacity
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // can only buffer 100 messages at a time, otherwise drops msg on floor
            udp_capacity: 100,
            // can only buffer 100 messages at a time, otherwise drops msg on floor
            ip_hop_capacity: 100,
            nic_capacity: 100000,
            // granularity of tick(), it's necessary to tick to simulate clock skew between
            // hosts
            tick_amount: Duration::from_millis(10),
            latency: Default::default(),
            message_loss: Default::default(),
        }
    }
}
