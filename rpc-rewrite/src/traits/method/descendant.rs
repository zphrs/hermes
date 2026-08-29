use crate::traits::method::{Method, ReqOf, ResOf};
pub trait Descendant<Parent: Method>: Method {
    fn req_from_parent<'buf>(req: ReqOf<'buf, Parent>) -> ReqOf<'buf, Self>;
    fn res_to_parent<'buf>(res: ResOf<'buf, Self>) -> ResOf<'buf, Parent>;
}
