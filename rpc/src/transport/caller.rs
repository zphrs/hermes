pub mod ext;

pub use ext::CallerExt;

use super::BiStream;

pub trait Caller: BiStream + Sized {
    type Error;
    fn open_stream(
        &self,
    ) -> impl Future<Output = Result<(Self::SendStream, Self::RecvStream), Self::Error>>;
}
