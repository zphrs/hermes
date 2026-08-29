pub struct True;
pub struct False;

pub(super) trait Sealed {}

impl Sealed for True {}
impl Sealed for False {}
