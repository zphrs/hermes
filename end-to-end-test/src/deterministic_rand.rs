use std::sync::LazyLock;

use rand::{RngCore, SeedableRng as _, rngs::StdRng};

#[allow(dead_code)]
fn fill_bytes(buf: &mut [u8]) -> std::result::Result<(), getrandom::Error> {
    static RNG: LazyLock<std::sync::Mutex<StdRng>> =
        LazyLock::new(|| StdRng::seed_from_u64(0x5EEDu64).into());
    RNG.lock().unwrap().fill_bytes(buf);
    Ok(())
}

#[cfg(test)]
mod test {

    #[test]
    fn test_fill_bytes() {
        let mut buf = [0u8; 32];
        getrandom::fill(&mut buf).unwrap();
        expect_test::expect!["[1b, 8c, 20, cd, e2, db, b4, 3c, d3, c7, 9, b2, 90, ac, 50, dc, d2, be, 2a, 87, a3, a2, 45, 44, b5, a5, 10, 9b, c7, 6e, a7, fb]"]
        .assert_eq(&format!("{:0x?}", buf));

        getrandom::fill(&mut buf).unwrap();
        expect_test::expect!["[7, a6, 65, f1, 52, 91, 40, c, bd, 22, eb, 4a, d3, f4, db, 72, fd, 59, 3d, e1, 3b, b6, 4b, 6, 71, fe, 12, 57, 51, 7a, e3, bd]"]
        .assert_eq(&format!("{:0x?}", buf));
    }
}
