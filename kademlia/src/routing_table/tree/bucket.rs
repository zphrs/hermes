use std::fmt::Debug;

use crate::helpers::from_fn;

use crate::id::DistancePair;

pub struct Bucket<Node, const ID_LEN: usize, const BUCKET_SIZE: usize>(
    arrayvec::ArrayVec<DistancePair<Node, ID_LEN>, BUCKET_SIZE>,
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
    /// Instance of [Self::remove_nodes_where]. Removes a single node from
    /// the Bucket. Ideally, it is better to remove multiple nodes at once
    /// since each removal call requires a new allocation of a buffer.
    #[cfg(test)]
    pub fn remove_pair(&mut self, node: &DistancePair<Node, ID_LEN>) {
        self.remove_nodes_where(|n| n == node);
    }
}

impl<Node: Eq, const ID_LEN: usize, const BUCKET_SIZE: usize> Bucket<Node, ID_LEN, BUCKET_SIZE> {
    pub(crate) fn new() -> Self {
        Self(Default::default())
    }
    pub fn is_full(&self) -> bool {
        self.0.is_full()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// adds the new pair in if there is space available, otherwise does nothing
    pub fn add(&mut self, value: impl Into<DistancePair<Node, ID_LEN>>) {
        let _ = self.0.try_push(value.into());
    }
    /// Returns an iterator over the elements in the ringbuffer,
    /// removing [nodes](Node) as they are iterated over.
    pub(crate) fn drain(&mut self) -> impl Iterator<Item = DistancePair<Node, ID_LEN>> {
        self.0.drain(..)
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &DistancePair<Node, ID_LEN>> {
        self.0.iter()
    }

    pub fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = &mut DistancePair<Node, ID_LEN>> {
        self.0.iter_mut()
    }
    /// Removes all nodes which match the predicate.
    pub fn remove_nodes_where<F: FnMut(&DistancePair<Node, ID_LEN>) -> bool>(
        &mut self,
        mut predicate: F,
    ) {
        self.0.retain(|v| !predicate(v));
    }
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, str::FromStr};

    use super::*;
    use crate::{HasId as _, node::Node};
    use expect_test::expect;
    #[test]
    pub fn test_bucket() {
        let mut bucket = Bucket::<Node, 32, 2>::new();
        let local_node = Node::new(SocketAddr::from_str("0.0.0.1:8080").unwrap());
        let n1 = Node::new(SocketAddr::from_str("127.0.0.1:8080").unwrap());
        let n1_pair: DistancePair<_, _> = (n1.clone(), local_node.id()).into();
        bucket.add(n1_pair.clone());
        expect![[r#"
            Bucket(
                [
                    DistancePair(
                        FCBE...E762,
                        Node {
                            addr: 127.0.0.1:8080,
                            id: "Id(07D4...182D)",
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
                        FCBE...E762,
                        Node {
                            addr: 127.0.0.1:8080,
                            id: "Id(07D4...182D)",
                        },
                    ),
                    DistancePair(
                        65F7...A5BC,
                        Node {
                            addr: 0.0.0.0:8080,
                            id: "Id(9E9D...5AF3)",
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
                        FCBE...E762,
                        Node {
                            addr: 127.0.0.1:8080,
                            id: "Id(07D4...182D)",
                        },
                    ),
                    DistancePair(
                        65F7...A5BC,
                        Node {
                            addr: 0.0.0.0:8080,
                            id: "Id(9E9D...5AF3)",
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
                        FCBE...E762,
                        Node {
                            addr: 127.0.0.1:8080,
                            id: "Id(07D4...182D)",
                        },
                    ),
                    DistancePair(
                        65F7...A5BC,
                        Node {
                            addr: 0.0.0.0:8080,
                            id: "Id(9E9D...5AF3)",
                        },
                    ),
                ],
            )
        "#]]
        .assert_debug_eq(&bucket);

        bucket.remove_nodes_where(|n| *n == n2_pair);
        expect![[r#"Bucket([DistancePair(FCBE...E762, Node { addr: 127.0.0.1:8080, id: "Id(07D4...182D)" })])"#]].assert_eq(&format!("{:?}", bucket));
        bucket.add(n1_pair);
        bucket.add(n2_pair);
        expect![[r#"
            [
                DistancePair(
                    FCBE...E762,
                    Node {
                        addr: 127.0.0.1:8080,
                        id: "Id(07D4...182D)",
                    },
                ),
                DistancePair(
                    FCBE...E762,
                    Node {
                        addr: 127.0.0.1:8080,
                        id: "Id(07D4...182D)",
                    },
                ),
            ]
        "#]]
        .assert_debug_eq(&bucket.drain().collect::<Vec<_>>());
        expect!["Bucket([])"].assert_eq(&format!("{:?}", bucket));
    }
}
