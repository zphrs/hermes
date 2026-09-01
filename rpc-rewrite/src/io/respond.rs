use minicbor::Decode;

use crate::traits::{
    BranchHandler, Replier, handler,
    io::BytesReadStream,
    method::{self, ReqOf, ResOf},
    replier,
};

use super::utilities::read_to_end;

#[derive(Debug, thiserror::Error)]
pub enum Error<Replier, Read> {
    #[error("read: {0}")]
    Read(Read),
    #[error("decode: {0}")]
    Decode(#[from] minicbor::decode::Error),
    #[error("handler: {0}")]
    Replier(Replier),
}

pub(crate) async fn respond<
    'buf,
    RootMethod: method::Branch + method::Loopback,
    H: BranchHandler<RootMethod>,
    R: Replier<RootMethod>,
    B: BytesReadStream,
>(
    buf: &'buf mut Vec<u8>,
    recv: B,
    replier: R,
    handler: &mut H,
) -> Result<R::Receipt<ResOf<'buf, RootMethod>>, Error<R::Error, B::Error>>
where
    ReqOf<'buf, RootMethod>: Decode<'buf, ()>,
{
    buf.clear();
    read_to_end(recv, buf, usize::MAX)
        .await
        .map_err(Error::Read)?;

    let request: ReqOf<RootMethod> = minicbor::decode(buf)?;
    let res = handler
        .handle::<R>(request, replier)
        .await
        .map_err(Error::Replier)?;
    Ok(res)
}

pub(crate) async fn transition<
    'buf,
    RootMethod: method::Branch + method::Transitions,
    H: handler::transition::BranchHandler<RootMethod>,
    R: replier::transition::Replier<RootMethod>,
    B: BytesReadStream,
>(
    buf: &'buf mut Vec<u8>,
    recv: B,
    replier: R,
    handler: H,
) -> Result<(R::Receipt<ResOf<'buf, RootMethod>>, H::NextHandler), Error<R::Error, B::Error>>
where
    ReqOf<'buf, RootMethod>: Decode<'buf, ()>,
{
    buf.clear();
    read_to_end(recv, buf, usize::MAX)
        .await
        .map_err(Error::Read)?;

    let request: ReqOf<RootMethod> = minicbor::decode(buf)?;
    let res = handler
        .handle_transition::<R>(request, replier)
        .await
        .map_err(Error::Replier)?;
    Ok(res)
}
