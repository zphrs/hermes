#[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen, maxlen::MaxLen)]
pub struct Req {
    #[n(0)]
    pub username: max_sized_string::MaxSizedString<256>,
    #[n(1)]
    pub password: max_sized_string::MaxSizedString<256>,
}
