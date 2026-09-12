use crate::marker::NotApplicable;
use crate::{Method, method};

pub(crate) struct ProcessorSacrificed;

impl Method for ProcessorSacrificed {
    type Req<'buf> = ();

    type Res<'buf> = NotApplicable;

    type Type = method::LeafTransition;
}
