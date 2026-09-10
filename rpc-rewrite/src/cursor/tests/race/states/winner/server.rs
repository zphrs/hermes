use crate::{cursor, markers::NotApplicable};

pub struct State;

impl cursor::State for State {
    type ClientHandles = NotApplicable;
    type ServerHandles = NotApplicable;
}
