use crate::Method;
use crate::marker::{False, NotApplicable};

pub(crate) struct ProcessorSacrificed;

impl Method for ProcessorSacrificed {
    type Req<'buf> = ();

    type Res<'buf> = NotApplicable;

    type Transitions = False;

    type HasDescendants = False;
}
