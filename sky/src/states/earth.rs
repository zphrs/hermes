use rpc::method::not_applicable::NotApplicable;

pub struct State;

// TODO: fill in requests for earth
impl rpc::State for State {
    type ClientHandles = NotApplicable;

    type ServerHandles = NotApplicable;
}
