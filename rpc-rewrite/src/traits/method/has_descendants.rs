pub(super) trait Sealed {}

pub struct True;
pub struct False;
impl Sealed for True {}
impl Sealed for False {}
