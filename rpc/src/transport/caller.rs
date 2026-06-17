pub mod ext;

pub use ext::CallerExt;

use super::BiStream;

pub trait Caller: BiStream + Sized {
    type Error;
    type OpenStreamFut: Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::Error>>
        + Unpin;
    fn open_stream(&self) -> Self::OpenStreamFut;
}
