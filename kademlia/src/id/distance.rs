use std::fmt::{Debug, Display, from_fn};
use std::ops::{Add, BitXor, Index, Shl, Shr};

use crate::HasId;

use super::Id;

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct Distance<const N: usize>([u8; N]);

impl<const N: usize> Distance<N> {
    pub const ONE: Distance<N> = {
        let mut arr = [0u8; N];
        if N > 0 {
            arr[N - 1] = 1;
        }
        Distance(arr)
    };
    pub const ZERO: Distance<N> = Distance([0u8; N]);
    pub const MAX: Distance<N> = Distance([u8::MAX; N]);

    pub fn leading_zeros(&self) -> usize {
        let mut count = 0;
        for &byte in &self.0 {
            if byte == 0 {
                count += 8;
            } else {
                count += byte.leading_zeros() as usize;
                break;
            }
        }
        count
    }
}

impl<const N: usize> Add for &Distance<N> {
    type Output = Distance<N>;

    fn add(self, rhs: Self) -> Self::Output {
        let mut out = [0u8; N];
        let mut carry = 0u8;

        for i in (0..N).rev() {
            let sum = self.0[i] as u16 + rhs.0[i] as u16 + carry as u16;
            out[i] = sum as u8;
            carry = (sum >> 8) as u8;
        }

        Distance(out)
    }
}

impl<const N: usize> BitXor for &Id<N> {
    type Output = Distance<N>;

    fn bitxor(self, rhs: Self) -> Self::Output {
        let mut out = [0u8; N];
        for (i, (left, right)) in self.0.iter().zip(rhs.0.iter()).enumerate() {
            out[i] = *left ^ *right;
        }
        Distance(out)
    }
}

impl<const N: usize> Shl<usize> for Distance<N> {
    type Output = Distance<N>;

    fn shl(self, rhs: usize) -> Self::Output {
        let byte_shift = rhs / 8;
        let bit_shift = rhs % 8;

        if byte_shift >= N {
            return Distance([0u8; N]);
        }

        let mut out = [0u8; N];

        // Shift bytes
        out[0..(N - byte_shift)].copy_from_slice(&self.0[byte_shift..N]);

        // Shift bits within bytes
        if bit_shift > 0 {
            let mut carry = 0u8;
            for out_i in &mut out[0..N] {
                let new_carry = *out_i >> (8 - bit_shift);
                *out_i = (*out_i << bit_shift) | carry;
                carry = new_carry;
            }
        }

        Distance(out)
    }
}

impl<const N: usize> Shr<usize> for Distance<N> {
    type Output = Distance<N>;

    fn shr(self, rhs: usize) -> Self::Output {
        let byte_shift = rhs / 8;
        let bit_shift = rhs % 8;

        if byte_shift >= N {
            return Distance([0u8; N]);
        }

        let mut out = [0u8; N];

        // Shift bytes
        out[byte_shift..N].copy_from_slice(&self.0[..(N - byte_shift)]);

        // Shift bits within bytes
        if bit_shift > 0 {
            let mut carry = 0u8;
            for i in (0..N).rev() {
                let new_carry = out[i] & ((1 << bit_shift) - 1);
                out[i] = (out[i] >> bit_shift) | (carry << (8 - bit_shift));
                carry = new_carry;
            }
        }

        Distance(out)
    }
}

impl<const N: usize> BitXor<&Distance<N>> for &Id<N> {
    type Output = Id<N>;

    fn bitxor(self, rhs: &Distance<N>) -> Self::Output {
        let mut out = [0u8; N];
        for (i, (left, right)) in self.0.iter().zip(rhs.0.iter()).enumerate() {
            out[i] = *left ^ *right;
        }
        Id(out)
    }
}

impl<const N: usize> BitXor<&Id<N>> for &Distance<N> {
    type Output = Id<N>;

    fn bitxor(self, rhs: &Id<N>) -> Self::Output {
        let mut out = [0u8; N];
        for (i, (left, right)) in self.0.iter().zip(rhs.0.iter()).enumerate() {
            out[i] = *left ^ *right;
        }
        Id(out)
    }
}

impl<const N: usize> Index<usize> for Distance<N> {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        self.0.index(index)
    }
}

impl<const N: usize> Debug for Distance<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let as_id: Id<N> = self.0.into();
        let first_zero_offset = as_id.get_first_zero_byte();
        if first_zero_offset == 0 {
            return write!(f, "Distance()");
        }
        f.debug_tuple("Distance")
            .field(&from_fn(|f| as_id.debug_id_bytes(first_zero_offset, f)))
            .finish()
    }
}

impl<const N: usize> Display for Distance<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let as_id: Id<N> = self.0.into();

        as_id.display_id_bytes(f)
    }
}

#[derive(Clone)]
pub struct Pair<Node, const ID_LEN: usize>(Distance<ID_LEN>, Node);

impl<Node: Eq, const ID_LEN: usize> Pair<Node, ID_LEN> {
    pub fn distance(&self) -> &Distance<ID_LEN> {
        &self.0
    }

    pub fn node(&self) -> &Node {
        &self.1
    }
    pub fn into_parts(self) -> (Distance<ID_LEN>, Node) {
        (self.0, self.1)
    }

    pub fn into_node(self) -> Node {
        self.1
    }

    pub fn from_parts((dist, node): (Distance<ID_LEN>, Node)) -> Self {
        Self(dist, node)
    }
}

impl<Node: Debug, const ID_LEN: usize> Debug for Pair<Node, ID_LEN> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DistancePair")
            .field(&from_fn(|f| write!(f, "{}", self.0)))
            .field(&self.1)
            .finish()
    }
}

impl<Node: Eq, const ID_LEN: usize> From<(Distance<ID_LEN>, Node)> for Pair<Node, ID_LEN> {
    fn from(value: (Distance<ID_LEN>, Node)) -> Self {
        Self::from_parts(value)
    }
}

impl<Node: Eq, const ID_LEN: usize> From<Pair<Node, ID_LEN>> for (Distance<ID_LEN>, Node) {
    fn from(value: Pair<Node, ID_LEN>) -> Self {
        value.into_parts()
    }
}

impl<Node: Eq, const ID_LEN: usize> PartialEq for Pair<Node, ID_LEN> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.1 == other.1
    }
}

impl<Node: Eq, const ID_LEN: usize> Eq for Pair<Node, ID_LEN> where Node: HasId<ID_LEN> {}

impl<Node: Eq, const ID_LEN: usize> From<(Node, &Id<ID_LEN>)> for Pair<Node, ID_LEN>
where
    Node: HasId<ID_LEN>,
{
    fn from(value: (Node, &Id<ID_LEN>)) -> Self {
        Self(value.1.xor_distance(value.0.id()), value.0)
    }
}

impl<Node: Eq, const ID_LEN: usize> Ord for Pair<Node, ID_LEN>
where
    Node: HasId<ID_LEN>,
{
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

impl<Node: Eq, const ID_LEN: usize> PartialOrd for Pair<Node, ID_LEN>
where
    Node: HasId<ID_LEN>,
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
