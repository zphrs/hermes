use crate::traits::method::not_applicable::NotApplicable;

pub struct ClientEndpoint;

impl crate::State for ClientEndpoint {
    type ClientHandles = NotApplicable;

    type ServerHandles = NotApplicable;
}
