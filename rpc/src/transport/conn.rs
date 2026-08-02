pub trait Connection: super::Caller + super::Client + PartialEq {}

impl<T: super::Caller + super::Client + PartialEq> Connection for T {}
