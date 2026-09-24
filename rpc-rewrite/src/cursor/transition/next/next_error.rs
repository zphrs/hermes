use std::fmt::Debug;

use crate::{cursor::transition::next::commit_or_defer, io::Connection};

#[derive(thiserror::Error)]
pub enum NextError<C: Connection, FutErr> {
    #[error("read: {0}")]
    Read(#[from] crate::io::read::Error<C::RecvStream>),
    #[error("write: {0}")]
    Write(#[from] crate::io::write::Error<C::SendStream>),
    #[error("accept uni stream: {0}")]
    SendNotification(#[from] crate::io::notify::SendError<C>),
    #[error("open uni stream: {0}")]
    RecvNotification(#[from] crate::io::notify::RecvError<C>),
    #[error("peer rejected our transition request before we replied to theirs")]
    UnexpectedRejection,
    #[error("while resolving future: {0}")]
    Fut(FutErr),
}

impl<C: Connection, FutErr> From<commit_or_defer::Error<C>> for NextError<C, FutErr> {
    fn from(value: commit_or_defer::Error<C>) -> Self {
        use commit_or_defer::Error as COD;
        match value {
            COD::Read(error) => Self::Read(error),
            COD::Write(error) => Self::Write(error),
            COD::SendNotification(error) => Self::SendNotification(error),
            COD::ReceiveNotification(error) => Self::RecvNotification(error),
            COD::UnexpectedRejection => Self::UnexpectedRejection,
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
            Self::UnexpectedRejection => write!(f, "UnexpectedRejection"),
            Self::Fut(arg0) => f.debug_tuple("Fut").field(arg0).finish(),
        }
    }
}
