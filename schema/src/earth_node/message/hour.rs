use std::fmt::Debug;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use std::ops::Div;

use std::ops::Sub;
use std::thread;
use std::time::Duration;

#[derive(PartialEq, PartialOrd, Eq, Ord)]
pub struct Hour(u64);

impl Debug for Hour {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}h", self.0)
    }
}

#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
#[cbor(transparent)]
pub struct SinceEpoch(u64);
impl Sub for Hour {
    type Output = Hour;

    fn sub(self, rhs: Hour) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl Div<Hour> for SystemTime {
    type Output = SinceEpoch;

    fn div(self, rhs: Hour) -> Self::Output {
        const SECS_IN_HOUR: u64 = 60 * 60;
        SinceEpoch(
            (self
                .duration_since(UNIX_EPOCH)
                .unwrap_or_else(|_| SystemTime::now().duration_since(UNIX_EPOCH).unwrap())
                .as_secs())
                / (SECS_IN_HOUR * rhs.0),
        )
    }
}

impl SinceEpoch {
    pub fn abs_diff(&self, other: Self) -> Hour {
        Hour(self.0.abs_diff(other.0))
    }
}

impl Sub for SinceEpoch {
    type Output = Hour;

    fn sub(self, rhs: Self) -> Self::Output {
        Hour(self.0 - rhs.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_difference_calculation() {
        let sent_at_hour = SystemTime::now() / Hour(1);

        let current_hour =
            (SystemTime::now() + Duration::from_secs((3600.0 * 1.55) as u64)) / Hour(1);

        assert!(current_hour.abs_diff(sent_at_hour) <= Hour(1));
    }
}
