pub mod client;
pub mod server;

pub(crate) mod dirs;
pub mod error;
pub(crate) mod get_public_ip;
pub mod no_cert_verification;
pub mod quinn_transport;

mod get_system_time;
#[cfg(test)]
mod kad_test;
pub mod node;
pub mod request_handler;
#[cfg(test)]
mod tokio_uptime;

#[cfg(test)]
pub mod rand_test {

    #[test]
    pub fn test() {
        let mut arr = [0u8; 32];
        getrandom2::getrandom(&mut arr).unwrap();
        let expect = expect_test::expect![
            "[27, 140, 32, 205, 226, 219, 180, 60, 211, 199, 9, 178, 144, 172, 80, 220, 210, 190, 42, 135, 163, 162, 69, 68, 181, 165, 16, 155, 199, 110, 167, 251]"
        ];
        let arr_as_string = &format!("{arr:?}");
        expect.assert_eq(arr_as_string);
        getrandom3::fill(&mut arr).unwrap();
        expect.assert_eq(arr_as_string);
        getrandom4::fill(&mut arr).unwrap();
        expect.assert_eq(arr_as_string);
    }
}
