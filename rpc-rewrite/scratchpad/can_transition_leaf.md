# Can Transition Leaf

Branches can currently contain both Leaves that Transition and Leaves that do not
transition. The question here is whether it should be possible to have a Leaf
handler that decides at runtime whether or not to transition.

The API would require Requesters of such a Method to pass in an owned Requester.

The API would NOT require Processors of such a Method to have exclusive 
processing of this request. Instead, the request could be processed in parallel
and only at the end of processing would it be decided whether the Processor
is indeed initiating a transition or not (based on whether or not it constructed
a state::Wrapper).
