use std::convert::Infallible;

use arrayvec::ArrayVec;
use max_sized_vec::MaxSizedVec;
use shared_schema::{EarthNode, earth_node::EarthId};

use crate::api::earth_root::register::Candidates;

use super::{OnlineNode, OnlineNodes};

pub type Request = EarthId;
pub type Response = MaxSizedVec<(EarthNode, Candidates), 10>;

pub struct Method<'a> {
    pub(super) map: &'a OnlineNodes,
}

impl<'a> rpc::Method for Method<'a> {
    type Req = Request;

    type Res = Response;

    type Error = Infallible;
}

impl<'a> Method<'a> {
    fn mapped_iter<'b>(
        iter: impl Iterator<Item = (&'b EarthId, &'b OnlineNode)>,
    ) -> std::iter::Map<
        impl Iterator<Item = (&'b EarthId, &'b OnlineNode)>,
        impl FnMut((&'b EarthId, &'b OnlineNode)) -> (EarthNode, Candidates),
    > {
        iter.map(|(_, entry)| (entry.remote.clone(), entry.connection_candidates.clone()))
    }
    fn call_inner(&self, earth_id: EarthId) -> <Self as rpc::Method>::Res {
        let btree_map = self.map.read();
        if btree_map.len() < 10 {
            return MaxSizedVec::from_iter(Self::mapped_iter(btree_map.iter()));
        }
        let to_max = btree_map.range(&earth_id..);
        let mut ten_greater = ArrayVec::<_, 10>::new();
        ten_greater.extend(Self::mapped_iter(to_max.take(10)));

        if ten_greater.len() < 10 {
            let from_min = btree_map.range(..);
            let smallest_id_seen = ten_greater.get(0).map(|v| v.0.earth_id().clone());
            ten_greater.extend(Self::mapped_iter(from_min.take(10).take_while(
                |(id, _)| {
                    // if we hit the first id in the other array then we've wrapped around
                    // and we should short circuit
                    Some(*id) != smallest_id_seen.as_ref()
                },
            )));
        }

        let to_min = btree_map.range(..&earth_id).rev();
        let mut ten_lesser = ArrayVec::<_, 10>::new();
        ten_lesser.extend(Self::mapped_iter(to_min.take(10)));

        if ten_lesser.len() < 10 {
            let from_max = btree_map.range(..).rev();
            let smallest_id_seen = ten_lesser.get(0).map(|v| v.0.earth_id().clone());
            ten_lesser.extend(from_max.take(10).map_while(|(id, entry)| {
                // if we hit the first id in the other array then we've wrapped around
                // and we should short circuit
                (Some(id) != smallest_id_seen.as_ref())
                    .then(|| (entry.remote.clone(), entry.connection_candidates.clone()))
            }));
        }

        MaxSizedVec::from_iter(ten_lesser.into_iter().rev().chain(ten_greater.into_iter()))
    }
}

impl rpc::Call for Method<'_> {
    async fn call<T: futures_io::AsyncWrite + Unpin + Send + Sync, TransportError>(
        &mut self,
        replier: rpc::Replier<'_, T, Self>,
        value: Self::Req,
    ) -> Result<rpc::ReplyReceipt<Self::Res>, rpc::ClientError<TransportError, Self::Error>> {
        replier.reply(self.call_inner(value)).await
    }
}
