use crate::traits::markers::{False, True};

pub(super) trait Sealed {}

impl Sealed for True {}
impl Sealed for False {}
