pub trait Connection: super::Caller + super::Client {}

impl<T: super::Caller + super::Client> Connection for T {}
