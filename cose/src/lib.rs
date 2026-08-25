pub mod types;

#[cfg(test)]
mod tests {
    pub fn diag_to_bytes(text: &str) -> Vec<u8> {
        cbor_diag::parse_diag(text).unwrap().to_bytes()
    }

    #[test]
    fn single_signature() {
        const DIAG: &str = {
            r#"
        98(
          [
            / protected / h'',
            / unprotected / {},
            / payload / 'This is the content.',
            / signatures / [
              [
                / protected h'a10126' / << {
                    / alg / 1:-7 / ECDSA 256 /
                  } >>,
                / unprotected / {
                  / kid / 4:'11'
                },
                / signature / h'e2aeafd40d69d19dfe6e52077c5d7ff4e408282cbefb
        5d06cbf414af2e19d982ac45ac98b8544c908b4507de1e90b717c3d34816fe926a2b
        98f53afd2fa0f30a'
              ]
            ]
          ]
        )
        "#
        };
        let bytes = diag_to_bytes(DIAG);
    }
}
