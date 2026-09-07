use crate::{
    cursor::transition::RequesterTransitionEntrypoint,
    traits::{self, Method, markers::NotApplicable, method::ReqOf},
};

use super::super::requester;

pub enum RequesterOrRequesterTransition<
    'buf,
    'req,
    State,
    Role,
    RootMethod: Method,
    C: traits::Connection,
    M,
> {
    Requester(requester::Requester<State, Role, RootMethod, C>),
    RequesterTransition(
        RequesterTransitionEntrypoint<'buf, State, Role, C, ReqOf<'req, RootMethod>, M>,
    ),
}

impl<'buf, 'req, State, Role, RootMethod: Method, C: traits::Connection>
    From<requester::Requester<State, Role, RootMethod, C>>
    for RequesterOrRequesterTransition<'buf, 'req, State, Role, RootMethod, C, NotApplicable>
{
    fn from(value: requester::Requester<State, Role, RootMethod, C>) -> Self {
        Self::Requester(value)
    }
}

impl<'buf, 'req, State, Role, RootMethod: Method, C: traits::Connection>
    RequesterOrRequesterTransition<'buf, 'req, State, Role, RootMethod, C, NotApplicable>
{
    pub async fn immediate_requester(
        requester: requester::Requester<State, Role, RootMethod, C>,
    ) -> Self {
        Self::Requester(requester)
    }
}

impl<'buf, 'req, State, Role, RootMethod: Method, C: traits::Connection, M>
    RequesterOrRequesterTransition<'buf, 'req, State, Role, RootMethod, C, M>
{
    pub fn conn(&self) -> &C {
        match self {
            RequesterOrRequesterTransition::Requester(requester) => requester.conn(),
            RequesterOrRequesterTransition::RequesterTransition(requester_transition) => {
                requester_transition.conn()
            }
        }
    }
}
