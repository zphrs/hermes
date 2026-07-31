use crate::traits::method::not_applicable::NotApplicable;

pub struct ClientEndpoint;

impl crate::State for ClientEndpoint {
    type ClientMethod = NotApplicable;

    type ServerMethod = NotApplicable;
}
