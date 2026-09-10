use crate::cursor;
use crate::marker::NotApplicable;

pub struct State;

impl cursor::State for State {
    type ClientHandles = NotApplicable;

    type ServerHandles = NotApplicable;
}
