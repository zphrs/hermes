use crate::{
    cursor::transition::RequesterTransitionEntrypoint,
    io,
    marker::NotApplicable,
    method::{Method, ReqOf},
};

use super::super::requester;

pub enum RequesterOrRequesterTransition<
    'buf,
    'req,
    State,
    Role,
    RootMethod: Method,
    C: io::Connection,
    M,
> {
    Requester(requester::Requester<State, Role, RootMethod, C>),
    RequesterTransition(
        RequesterTransitionEntrypoint<'buf, State, Role, C, ReqOf<'req, RootMethod>, M>,
    ),
}

impl<'buf, 'req, State, Role, RootMethod: Method, C: io::Connection>
    From<requester::Requester<State, Role, RootMethod, C>>
    for RequesterOrRequesterTransition<'buf, 'req, State, Role, RootMethod, C, NotApplicable>
{
    fn from(value: requester::Requester<State, Role, RootMethod, C>) -> Self {
        Self::Requester(value)
    }
}

impl<'buf, 'req, State, Role, RootMethod: Method, C: io::Connection>
    RequesterOrRequesterTransition<'buf, 'req, State, Role, RootMethod, C, NotApplicable>
{
    pub async fn immediate_requester(
        requester: requester::Requester<State, Role, RootMethod, C>,
    ) -> Self {
        Self::Requester(requester)
    }
}

impl<'buf, 'req, State, Role, RootMethod: Method, C: io::Connection, M>
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
