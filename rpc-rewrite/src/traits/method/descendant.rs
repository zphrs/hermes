use crate::traits::method::{Method, ReqOf, ResOf};
pub trait Descendant<Parent: Method>: Method {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, Parent>;
    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, Parent>;
}

impl<M: Method> Descendant<M> for M {
    fn req_to_parent<'buf>(req: ReqOf<'buf, Self>) -> ReqOf<'buf, M> {
        req
    }

    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, M> {
        res
    }
}
