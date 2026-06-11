//! Marker types for each side
#[derive(Clone, Copy)]
pub struct Client;
#[derive(Clone, Copy)]
pub struct Server;

pub(crate) trait Role: Copy {}

impl Role for Client {}

impl Role for Server {}
