use super::skip_server_verification::SkipServerVerification;
use quinn::{ClientConfig, TransportConfig, VarInt, crypto::rustls::QuicClientConfig, rustls};
use std::sync::Arc;

/// Builds default quinn client config and trusts given certificates.
///
/// ## Args
///
/// - server_certs: a list of trusted certificates in DER format.
pub fn configure_client() -> anyhow::Result<ClientConfig> {
    let mut out = ClientConfig::new(Arc::new(QuicClientConfig::try_from(
        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_no_client_auth(),
    )?));

    let mut tp_config = TransportConfig::default();
    tp_config.max_concurrent_uni_streams(1u32.into());

    out.transport_config(tp_config.into());

    Ok(out)
}
