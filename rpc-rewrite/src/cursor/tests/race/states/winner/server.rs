use crate::{markers::NotApplicable, traits};

pub struct State;

impl traits::State for State {
    type ClientHandles = NotApplicable;
    type ServerHandles = NotApplicable;
}
