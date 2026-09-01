#[derive(Default)]
pub struct Buffers {
    pub read: Vec<u8>,
    pub write: Vec<u8>,
}

impl Buffers {
    pub fn new() -> Self {
        Self::default()
    }
}
