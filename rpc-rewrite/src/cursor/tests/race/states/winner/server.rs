use crate::{
    cursor,
    marker::{Loopback, NotApplicable},
};

pub struct State;

impl cursor::State for State {
    type ClientBranchType = Loopback;
    type ClientHandles = NotApplicable;

    type ServerBranchType = Loopback;
    type ServerHandles = NotApplicable;
}
