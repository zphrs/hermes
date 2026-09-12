use std::convert::Infallible;
mod replier {
    pub use super::super::super::*;
}

pub struct Receipt<Res>(pub(crate) Res);

impl<Res> replier::Receipt<Res> for Receipt<Res> {
    type Error = Infallible;

    async fn finalize(self) -> Result<Res, Self::Error> {
        Ok(self.0)
    }
}

impl<Res> Receipt<Res> {
    pub(crate) fn map<T>(self, mapper: impl FnOnce(Res) -> T) -> Receipt<T> {
        let Self(res) = self;
        Receipt(mapper(res))
    }
}
