use std::convert::Infallible;

use minicbor::{CborLen, Decode, Encode};

use crate::traits::{
    Connection, Method,
    io::{BytesReadStream, BytesWriteStream},
    method::{self, Descendant, ReqOf, ResOf},
};

use super::utilities::{read_to_end, write_all};

#[derive(Debug, thiserror::Error)]
pub enum Error<C: Connection> {
    #[error("encode: {0}")]
    Encode(#[from] minicbor::encode::Error<Infallible>),
    #[error("decode: {0}")]
    Decode(#[from] minicbor::decode::Error),
    #[error("write: {0}")]
    Write(<C::SendStream as BytesWriteStream>::Error),
    #[error("read: {0}")]
    Read(<C::RecvStream as BytesReadStream>::Error),
}

pub(crate) async fn request<
    'request,
    'buf,
    RootMethod: Method,
    M: method::Leaf + Descendant<RootMethod>,
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
    let root_method = M::req_to_parent(request);
    {
        let mut buf = Vec::with_capacity(minicbor::len(&root_method));
        minicbor::encode::<&ReqOf<'request, RootMethod>, _>(&root_method, &mut buf)?;
        write_all(send, buf.into(), false)
            .await
            .map_err(Error::Write)?;
    }

    read_to_end(recv, buf, usize::MAX)
        .await
        .map_err(Error::Read)?;
    // recv dropped here
    let res: ResOf<'_, M> = minicbor::decode(buf)?;
    Ok(res)
}
