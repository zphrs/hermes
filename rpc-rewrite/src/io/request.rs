use minicbor::{CborLen, Decode, Encode};

use crate::{
    io,
    method::{self, Descendant, ReqOf, ResOf},
};

use super::Connection;

#[derive(Debug, thiserror::Error)]
pub enum Error<C: Connection> {
    #[error("could not write request")]
    Read(#[from] io::read::Error<C::RecvStream>),
    #[error("could not read response")]
    Write(#[from] io::write::Error<C::SendStream>),
}

pub(crate) async fn request<
    'request,
    'buf,
    RootMethod: method::Method,
    M: method::Method + Descendant<RootMethod>,
    C: Connection,
>(
    buf: &'buf mut Vec<u8>,
    request: ReqOf<'request, M>,
    (send, recv): (C::SendStream, C::RecvStream),
) -> Result<ResOf<'buf, M>, Error<C>>
where
    ReqOf<'request, RootMethod>: CborLen<()> + Encode<()>,
    ResOf<'buf, M>: Decode<'buf, ()>,
{
    let root_request = M::req_to_parent(request);
    super::write::write(&root_request, send).await?;
    let res: ResOf<'_, M> = super::read(buf, recv).await?;
    Ok(res)
}
