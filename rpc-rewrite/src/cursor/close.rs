//! closing follows these steps:
//! 1. the client [`notify`](crate::io::notify)s the server with
//!    [`CloseRequested`] and waits for the connection to close
//! 2. the server receives [`CloseRequested`] and closes the connection
//!
//! The server closes because it is the side that received the last message.
//! Closing from the client could otherwise drop the notification (or any
//! other unread data) on the wire before the server reads it.

use crate::marker::NotApplicable;
use crate::{Method, io::Connection, method};

pub(crate) struct CloseRequested;

impl Method for CloseRequested {
    type Req<'buf> = ();

    type Res<'buf> = NotApplicable;

    type Type = method::LeafTransition;
}

#[derive(thiserror::Error)]
pub enum CloseError<C: Connection> {
    #[error("could not notify of close")]
    SendNotification(#[from] crate::io::notify::SendError<C>),
    #[error("while waiting for the server to close")]
    WaitForClose(#[source] C::WaitForCloseError),
}

impl<C: Connection> std::fmt::Debug for CloseError<C>
where
    C::OpenUniError: std::fmt::Debug,
    C::WaitForCloseError: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SendNotification(arg0) => f.debug_tuple("SendNotification").field(arg0).finish(),
            Self::WaitForClose(arg0) => f.debug_tuple("WaitForClose").field(arg0).finish(),
        }
    }
}

#[derive(thiserror::Error)]
pub enum WaitToCloseError<C: Connection> {
    #[error("while waiting for notification of close")]
    ReceiveNotification(#[from] crate::io::notify::RecvError<C>),
    #[error("could not close")]
    Close(#[source] C::CloseError),
}

impl<C: Connection> std::fmt::Debug for WaitToCloseError<C>
where
    C::AcceptUniError: std::fmt::Debug,
    C::CloseError: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReceiveNotification(arg0) => {
                f.debug_tuple("ReceiveNotification").field(arg0).finish()
            }
            Self::Close(arg0) => f.debug_tuple("Close").field(arg0).finish(),
        }
    }
}
