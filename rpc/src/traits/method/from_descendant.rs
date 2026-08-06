pub trait FromDescendant<Descendant: crate::Method>: crate::method::Ancestor<Descendant> {
    fn from_descendant_req(request: Descendant::Req) -> <Self as crate::Method>::Req;
    fn from_descendant_res(result: Descendant::Res) -> <Self as crate::Method>::Res;

    fn try_into_descendant_req(request: Self::Req) -> Result<Descendant::Req, Self::Req>;
}

impl<Descendant: crate::Method> FromDescendant<Descendant> for Descendant {
    fn from_descendant_req(
        request: <Descendant as super::Method>::Req,
    ) -> <Self as crate::Method>::Req {
        request
    }

    fn from_descendant_res(
        result: <Descendant as super::Method>::Res,
    ) -> <Self as crate::Method>::Res {
        result
    }

    fn try_into_descendant_req(
        request: Self::Req,
    ) -> Result<<Descendant as super::Method>::Req, Self::Req> {
        Ok(request)
    }
}
