pub mod client;

pub(crate) mod dirs;
pub mod error;
pub(crate) mod get_public_ip;
pub mod no_cert_verification;
pub mod quinn_transport;

mod get_system_time;
#[cfg(test)]
mod kad_test;
mod listener;
pub mod node;
pub mod request_handler;
mod tokio_uptime;

#[cfg(test)]
pub mod rand_test {

    #[test]
    pub fn test() {
        let mut arr = [0u8; 32];
        getrandom2::getrandom(&mut arr).unwrap();
        let expect = expect_test::expect![[r#"
            [
                27,
                140,
                32,
                205,
                226,
                219,
                180,
                60,
                211,
                199,
                9,
                178,
                144,
                172,
                80,
                220,
                210,
                190,
                42,
                135,
                163,
                162,
                69,
                68,
                181,
                165,
                16,
                155,
                199,
                110,
                167,
                251,
            ]
        "#]];
        expect.assert_debug_eq(&arr);
        getrandom3::fill(&mut arr).unwrap();
        expect.assert_debug_eq(&arr);
        getrandom4::fill(&mut arr).unwrap();
        expect.assert_debug_eq(&arr);
    }
}
