#[inline(always)]
pub fn wrapping_sum(nums: impl IntoIterator<Item = u16>) -> u16 {
    let mut out: u16 = 0;
    for num in nums {
        out = out.wrapping_add(num);
    }
    out
}

pub fn checksum_arr<const LEN: usize>(arr: &[u8; LEN]) -> u16 {
    assert!(LEN.is_multiple_of(2));
    wrapping_sum(
        arr.chunks_exact(2)
            .map(|arr| u16::from_be_bytes([arr[0], arr[1]])),
    )
}

pub const VALID_CHECKSUM: u16 = 0xffff;
