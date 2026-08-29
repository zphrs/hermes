use super::skip_server_verification::SkipServerVerification;
use quinn::{ClientConfig, crypto::rustls::QuicClientConfig, rustls};
use std::sync::Arc;

/// Builds default quinn client config and trusts given certificates.
///
/// ## Args
///
/// - server_certs: a list of trusted certificates in DER format.
pub fn configure_client() -> anyhow::Result<ClientConfig> {
    let out = ClientConfig::new(Arc::new(QuicClientConfig::try_from(
        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_no_client_auth(),
    )?));

    Ok(out)
}
