use crate::traits::method::not_applicable::NotApplicable;

pub struct ServerEndpoint;

impl crate::State for ServerEndpoint {
    type ClientHandles = NotApplicable;

    type ServerHandles = NotApplicable;
}
