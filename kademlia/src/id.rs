mod distance;
mod logging;
use std::ops::BitXorAssign;
use std::ops::Index;

pub use self::distance::Distance;
pub use self::distance::Pair as DistancePair;
use crate::HasId;

#[derive(Clone, PartialEq, Eq)]
pub struct Id<const N: usize>([u8; N]);

impl<const N: usize> HasId<N> for Id<N> {
    fn id(&self) -> &Id<N> {
        self
    }
}

impl<const N: usize> HasId<N> for &Id<N> {
    fn id(&self) -> &Id<N> {
        self
    }
}
impl<const M: usize, const N: usize> From<[u8; M]> for Id<N> {
    fn from(value: [u8; M]) -> Self {
        assert!(
            M <= N,
            "invalid id length of {}, must be <= {} bytes long",
            M,
            N
        );
        // needed for type stuff
        let mut out = [0u8; N];
        out[..M].copy_from_slice(&value);
        Id(out)
    }
}

impl<const N: usize> BitXorAssign for Id<N> {
    fn bitxor_assign(&mut self, rhs: Self) {
        for (left, right) in self.0.iter_mut().zip(rhs.0.iter()) {
            *left ^= *right;
        }
    }
}

impl<const N: usize> Id<N> {
    /// The current bit length of addresses
    /// needs to be a constant size, can pad with zeroes for values which are
    /// less long than this size
    pub const BITS: usize = N * 8;

    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn xor_distance(&self, rhs: &Self) -> Distance<N> {
        self ^ rhs
    }
}

impl<const N: usize> Index<usize> for Id<N> {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        self.0.index(index)
    }
}

#[cfg(test)]
mod tests {

    use expect_test::expect;

    use super::*;

    #[test]
    pub fn id_logging() {
        let id = Id::<512>::from([255; 32]);

        expect!["Id(FFFF...FFFF)"].assert_eq(&format!("{}", id));

        expect!["Id(FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF)"]
            .assert_eq(&format!("{:?}", id));

        expect![[r#"
            Id(
                FFFFFFFFFFFFFFFF
                FFFFFFFFFFFFFFFF
                FFFFFFFFFFFFFFFF
                FFFFFFFFFFFFFFFF,
            )"#]]
        .assert_eq(&format!("{:#?}", id));
        expect!["Id()"].assert_eq(&format!("{}", Id::<512>::from([])));
        expect!["Id()"].assert_eq(&format!("{:?}", Id::<512>::from([])));
        expect!["Id()"].assert_eq(&format!("{:#?}", Id::<512>::from([])));
    }
}
