pub trait Method {
    type Req: Send;
    type Res: Send;
    /// used to abort a reply midway through handling a request
    type Error;
}
