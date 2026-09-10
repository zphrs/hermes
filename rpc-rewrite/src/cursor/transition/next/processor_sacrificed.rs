use crate::markers::{False, NotApplicable};
use crate::traits::Method;

pub(crate) struct ProcessorSacrificed;

impl Method for ProcessorSacrificed {
    type Req<'buf> = ();

    type Res<'buf> = NotApplicable;

    type Transitions = False;

    type HasDescendants = False;
}
