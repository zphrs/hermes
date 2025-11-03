use std::fmt::{Debug, from_fn};

use ringbuffer::{ConstGenericRingBuffer, RingBuffer as _};

use crate::{BUCKET_SIZE, HasId, id::DistancePair};

pub struct Bucket<Node, const ID_LEN: usize, const BUCKET_SIZE: usize>(
    ConstGenericRingBuffer<DistancePair<Node, ID_LEN>, BUCKET_SIZE>,
);

impl<Node: Debug + Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Debug
    for Bucket<Node, ID_LEN, BUCKET_SIZE>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Bucket")
            .field(&from_fn(|f| f.debug_list().entries(self.0.iter()).finish()))
            .finish()
    }
}

impl<Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Bucket<Node, ID_LEN, BUCKET_SIZE> {
    /// moves the used_node (if found) to the start of the new buffer
    /// otherwise is a no-op.
    pub fn mark_as_used(&mut self, node: &DistancePair<Node, ID_LEN>) {
        let bucket_dump = self.0.drain();
        let mut new_self = Self(Default::default());

        let mut used_node = None;
        let node_ref = node;
        for node in bucket_dump {
            if node == *node_ref {
                used_node = Some(node);
                continue;
            }
            new_self.add(node);
        }
        if let Some(used_node) = used_node {
            new_self.add(used_node);
        }

        *self = new_self
    }
    /// Instance of [Self::remove_nodes_where]. Removes a single node from
    /// the Bucket. Ideally, it is better to remove multiple nodes at once
    /// since each removal call requires a new allocation of a buffer.
    pub fn remove_pair(&mut self, node: &DistancePair<Node, ID_LEN>) {
        self.remove_nodes_where(|n| n == node);
    }
}

impl<Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Bucket<Node, ID_LEN, BUCKET_SIZE> {
    pub(crate) fn new() -> Self {
        return Self(ConstGenericRingBuffer::<
            DistancePair<Node, ID_LEN>,
            BUCKET_SIZE,
        >::new());
    }
    pub fn is_full(&self) -> bool {
        self.0.is_full()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// adds the new node in, potentially overwriting the least recently queued
    /// node from the bucket.
    pub fn add(&mut self, value: impl Into<DistancePair<Node, ID_LEN>>) {
        self.0.enqueue(value.into());
    }
    /// Returns an iterator over the elements in the ringbuffer,
    /// removing [nodes](Node) as they are iterated over.
    pub(crate) fn drain(&mut self) -> impl Iterator<Item = DistancePair<Node, ID_LEN>> {
        self.0.drain()
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &DistancePair<Node, ID_LEN>> {
        self.0.iter()
    }

    pub fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = &mut DistancePair<Node, ID_LEN>> {
        self.0.iter_mut()
    }
    /// Removes all nodes which match the predicate and returns the removed
    /// nodes. For example:
    /// ```
    /// # use kademlia::{Node, Bucket};
    /// pub fn remove_node(bucket: &mut Bucket, node: &Node) {
    ///   bucket.remove_nodes_where(|n| n == node);
    /// }
    /// ```
    pub fn remove_nodes_where<F: FnMut(&DistancePair<Node, ID_LEN>) -> bool>(
        &mut self,
        mut predicate: F,
    ) -> Vec<DistancePair<Node, ID_LEN>> {
        let mut new_self = Self(Default::default());
        let mut removed = Vec::new();
        for node in self.0.drain() {
            if (predicate)(&node) {
                // removed
                removed.push(node);
            } else {
                new_self.add(node);
            }
        }
        *self = new_self;
        return removed;
    }
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, str::FromStr};

    use super::*;
    use crate::node::Node;
    use expect_test::expect;
    #[test]
    pub fn test_bucket() {
        let mut bucket = Bucket::<Node, 256, 2>::new();
        let local_node = Node::new(SocketAddr::from_str("0.0.0.1:8080").unwrap());
        let n1 = Node::new(SocketAddr::from_str("127.0.0.1:8080").unwrap());
        let n1_pair: DistancePair<_, _> = (n1.clone(), local_node.id()).into();
        bucket.add(n1_pair.clone());
        expect![[r#"
            Bucket(
                [
                    DistancePair(
                        Distance(03B3...9EC4),
                        Node {
                            addr: 127.0.0.1:8080,
                            id: Id(1019...668C),
                        },
                    ),
                ],
            )
        "#]]
        .assert_debug_eq(&bucket);
        bucket.remove_pair(&n1_pair);
        expect!["Bucket([])"].assert_eq(&format!("{bucket:?}"));

        let n2 = Node::new(SocketAddr::from_str("0.0.0.0:8080").unwrap());
        let n2_pair: DistancePair<_, _> = (n2.clone(), local_node.id()).into();
        bucket.add(n1_pair.clone());
        bucket.add(n2_pair.clone());
        expect!["true"].assert_eq(&bucket.is_full().to_string());
        expect![[r#"
            Bucket(
                [
                    DistancePair(
                        Distance(03B3...9EC4),
                        Node {
                            addr: 127.0.0.1:8080,
                            id: Id(1019...668C),
                        },
                    ),
                    DistancePair(
                        Distance(0AD0...F026),
                        Node {
                            addr: 0.0.0.0:8080,
                            id: Id(197A...086E),
                        },
                    ),
                ],
            )
        "#]]
        .assert_debug_eq(&bucket);

        bucket.mark_as_used(&n1_pair);
        expect![[r#"
            Bucket(
                [
                    DistancePair(
                        Distance(0AD0...F026),
                        Node {
                            addr: 0.0.0.0:8080,
                            id: Id(197A...086E),
                        },
                    ),
                    DistancePair(
                        Distance(03B3...9EC4),
                        Node {
                            addr: 127.0.0.1:8080,
                            id: Id(1019...668C),
                        },
                    ),
                ],
            )
        "#]]
        .assert_debug_eq(&bucket);

        bucket.add(n2_pair.clone());
        expect![[r#"
            Bucket(
                [
                    DistancePair(
                        Distance(03B3...9EC4),
                        Node {
                            addr: 127.0.0.1:8080,
                            id: Id(1019...668C),
                        },
                    ),
                    DistancePair(
                        Distance(0AD0...F026),
                        Node {
                            addr: 0.0.0.0:8080,
                            id: Id(197A...086E),
                        },
                    ),
                ],
            )
        "#]]
        .assert_debug_eq(&bucket);

        bucket.add(n2_pair.clone());
        expect![[r#"
            Bucket(
                [
                    DistancePair(
                        Distance(0AD0...F026),
                        Node {
                            addr: 0.0.0.0:8080,
                            id: Id(197A...086E),
                        },
                    ),
                    DistancePair(
                        Distance(0AD0...F026),
                        Node {
                            addr: 0.0.0.0:8080,
                            id: Id(197A...086E),
                        },
                    ),
                ],
            )
        "#]]
        .assert_debug_eq(&bucket);

        bucket.remove_nodes_where(|n| *n == n2_pair);
        expect!["Bucket([])"].assert_eq(&format!("{:?}", bucket));
        bucket.add(n1_pair);
        bucket.add(n2_pair);
        expect![[r#"
            [
                DistancePair(
                    Distance(03B3...9EC4),
                    Node {
                        addr: 127.0.0.1:8080,
                        id: Id(1019...668C),
                    },
                ),
                DistancePair(
                    Distance(0AD0...F026),
                    Node {
                        addr: 0.0.0.0:8080,
                        id: Id(197A...086E),
                    },
                ),
            ]
        "#]]
        .assert_debug_eq(&bucket.drain().collect::<Vec<_>>());
        expect!["Bucket([])"].assert_eq(&format!("{:?}", bucket));
    }
}
