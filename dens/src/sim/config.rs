use std::{ops::DerefMut, time::Duration};

use rand::Rng;
use rand_distr::{Distribution as _, Exp};
use scoped_tls::scoped_thread_local;

scoped_thread_local!(pub(crate) static CONFIG: Config);

/// Configure how often messages are lost.
///
/// See [`MessageLoss::default()`](#method.default) for documentation on the default
/// values.
#[derive(Clone, Copy)]
pub struct MessageLoss {
    /// Probability of a message being dropped/corrupted
    pub fail_rate: f64,
}

impl MessageLoss {
    pub const ZERO: MessageLoss = MessageLoss { fail_rate: 0.0 };
    #[must_use]
    pub fn new(fail_rate: f64) -> Self {
        Self { fail_rate }
    }
    /// 0.5% packet loss, typical for residential ISP connections
    #[must_use]
    pub fn typical() -> Self {
        Self { fail_rate: 0.005 }
    }
    /// 2% packet loss, typical for mobile network connections
    #[must_use]
    pub fn degraded() -> Self {
        Self { fail_rate: 0.02 }
    }
    /// 10% packet loss, unusual and typically an indication that the underlying
    /// network is under extreme load.
    ///
    /// Such extreme packet loss can also be achieved by configuring the
    /// [`Config::ip_hop_capacity`](field@Config::ip_hop_capacity) to be
    /// extremely low or alternatively by oversaturating a single
    /// [`ip::Network`](crate::net::ip::Network).
    #[must_use]
    pub fn extreme() -> Self {
        Self { fail_rate: 0.1 }
    }
}

impl Default for MessageLoss {
    /// Default of typical (0.5%) packet loss
    fn default() -> Self {
        Self::typical()
    }
}

/// Configure latency behavior between two hosts.
///
/// See [`Latency::default()`](#method.default) for documentation on the default
/// values.
#[derive(Clone, Copy)]
pub struct Latency {
    /// Minimum latency.
    pub min_message_latency: Duration,

    /// Maximum latency.
    pub max_message_latency: Duration,

    /// Exponential probability distribution of latency within the range above.
    pub latency_distribution: Exp<f64>,
}

impl Latency {
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "having a duration greater than f64::MAX_EXACT_INTEGER is
        extremely unlikely and in such a case rounding is not an issue"
    )]
    pub fn sample<R>(&self, mut rand: impl DerefMut<Target = R>) -> Duration
    where
        R: Rng + ?Sized,
    {
        let sample = self.latency_distribution.sample(&mut *rand);
        let latency_ms = self.min_message_latency.as_millis() as f64 + sample;
        // No need to clamp with min_message_latency since we add it to the
        // sample above.
        let clamped = latency_ms.min(self.max_message_latency.as_millis() as f64);
        #[allow(clippy::cast_sign_loss, reason = "duration is always positive")]
        Duration::from_millis(clamped as u64)
    }
}
impl Default for Latency {
    /// Provides default values of:
    /// - `min_message_latency`: 10ms
    /// - `max_message_latency`: 1000ms
    /// - `latency_distribution`: Exp(50.0)
    fn default() -> Self {
        Self {
            min_message_latency: Duration::from_millis(10),
            max_message_latency: Duration::from_millis(1000),
            latency_distribution: Exp::new(50.0).unwrap(),
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
    #[must_use]
    pub fn latency(&self) -> &Latency {
        &self.latency
    }

    #[must_use]
    pub fn synchronous_network() -> Self {
        Self {
            tick_amount: Duration::from_millis(1),
            latency: Latency {
                min_message_latency: Duration::ZERO,
                max_message_latency: Duration::ZERO,
                #[expect(clippy::missing_panics_doc, reason = "infallible")]
                latency_distribution: Exp::new(1.0).unwrap(),
            },
            message_loss: MessageLoss::ZERO,
            ..Default::default()
        }
    }
    #[must_use]
    pub fn udp_capacity(&self) -> usize {
        self.udp_capacity
    }
    #[must_use]
    pub fn tick_amount(&self) -> Duration {
        self.tick_amount
    }

    #[must_use]
    pub fn message_loss_fail_rate(&self) -> f64 {
        self.message_loss.fail_rate
    }
    #[must_use]
    pub fn ip_hop_capacity(&self) -> usize {
        self.ip_hop_capacity
    }

    #[must_use]
    pub fn nic_capacity(&self) -> usize {
        self.nic_capacity
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // up to this many packets can be buffered in the OS, otherwise drops msg on floor
            udp_capacity: tokio::sync::Semaphore::MAX_PERMITS,
            // up to this many packets can be buffered in any network hop, otherwise drops msg on floor
            ip_hop_capacity: tokio::sync::Semaphore::MAX_PERMITS,
            // up to this many packets can be buffered in the Nic hop, otherwise drops msg on floor
            nic_capacity: tokio::sync::Semaphore::MAX_PERMITS,
            // granularity of tick(), it's necessary to tick to simulate clock skew between
            // hosts
            tick_amount: Duration::from_millis(1),
            latency: Latency::default(),
            message_loss: MessageLoss::default(),
        }
    }
}
