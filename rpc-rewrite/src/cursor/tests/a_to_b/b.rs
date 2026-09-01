use crate::traits::{self, markers::NotApplicable};

pub struct State;

impl traits::State for State {
    type ClientHandles = NotApplicable;

    type ServerHandles = NotApplicable;
}
