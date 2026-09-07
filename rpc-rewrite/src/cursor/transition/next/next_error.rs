use std::fmt::Debug;

use crate::{cursor::transition::next::definite_tiebreak, traits::Connection};

#[derive(thiserror::Error)]
pub enum NextError<C: Connection, FutErr> {
    #[error("read: {0}")]
    Read(#[from] crate::io::read::Error<C::RecvStream>),
    #[error("write: {0}")]
    Write(#[from] crate::io::write::Error<C::SendStream>),
    #[error("accept uni stream: {0}")]
    SendNotification(#[from] crate::io::notify::SendError<C>),
    #[error("open uni stream: {0}")]
    RecvNotification(#[from] crate::io::notify::ReceiveError<C>),
    #[error("while resolving future: {0}")]
    Fut(FutErr),
}

impl<C: Connection, FutErr> From<definite_tiebreak::Error<C>> for NextError<C, FutErr> {
    fn from(value: definite_tiebreak::Error<C>) -> Self {
        use definite_tiebreak::Error as DTE;
        match value {
            DTE::Read(error) => Self::Read(error),
            DTE::Write(error) => Self::Write(error),
            DTE::SendNotification(error) => Self::SendNotification(error),
            DTE::ReceiveNotification(error) => Self::RecvNotification(error),
        }
    }
}
impl<C: Connection, FutErr: Debug> Debug for NextError<C, FutErr>
where
    C::AcceptUniError: Debug,
    C::OpenUniError: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(arg0) => f.debug_tuple("Read").field(arg0).finish(),
            Self::Write(arg0) => f.debug_tuple("Write").field(arg0).finish(),
            Self::SendNotification(arg0) => f.debug_tuple("SendNotification").field(arg0).finish(),
            Self::RecvNotification(arg0) => f.debug_tuple("RecvNotification").field(arg0).finish(),
            Self::Fut(arg0) => f.debug_tuple("Fut").field(arg0).finish(),
        }
    }
}
