use std::{error::Error as StdError, net::SocketAddr, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use reqwest::{Certificate, Client, Response};
use rustls::{CertificateError, Error as RustlsError};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_RESPONSE_BYTES: usize = 1_024;
const MAX_ERROR_CHAIN_DEPTH: usize = 16;

#[derive(Clone, Copy, Debug)]
pub enum CertificateRejection {
    UnknownIssuer,
    NameMismatch,
}

pub fn client(ca_pem: &[u8], resolve: Option<(&str, SocketAddr)>) -> Result<Client> {
    let ca = Certificate::from_pem(ca_pem).context("invalid private test CA PEM")?;
    let mut builder = Client::builder()
        .tls_certs_only([ca])
        .no_proxy()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none());
    if let Some((host, address)) = resolve {
        builder = builder.resolve(host, address);
    }
    builder
        .build()
        .context("could not build strict localhost TLS client")
}

pub fn no_trusted_roots_client() -> Result<Client> {
    Client::builder()
        .tls_certs_only(std::iter::empty())
        .no_proxy()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("could not build client with an empty trust store")
}

pub async fn request(
    client: &Client,
    host: &str,
    port: u16,
    path: &str,
) -> reqwest::Result<Response> {
    client
        .get(format!("https://{host}:{port}{path}"))
        .send()
        .await
}

pub async fn get_text(client: &Client, host: &str, port: u16, path: &str) -> Result<(u16, String)> {
    let mut response = request(client, host, port, path).await?;
    let status = response.status().as_u16();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            body.len() + chunk.len() <= MAX_RESPONSE_BYTES,
            "HTTPS fixture response exceeded its bound"
        );
        body.extend_from_slice(&chunk);
    }
    Ok((status, String::from_utf8(body)?))
}

pub async fn check_tls_endpoints(
    tls_client: &Client,
    ca_pem: &[u8],
    host: &str,
    port: u16,
    nonce: &str,
) -> Result<()> {
    ensure!(host == "localhost", "mapped host must be localhost");
    let (status, body) = get_text(tls_client, host, port, "/healthz").await?;
    ensure!(
        status == 200 && body == "ok",
        "localhost HTTPS health response was unexpected"
    );
    let (status, body) = get_text(tls_client, host, port, "/nonce").await?;
    ensure!(
        status == 200 && body == nonce,
        "localhost HTTPS nonce response did not match"
    );

    let no_roots = no_trusted_roots_client()?;
    let result = request(&no_roots, host, port, "/nonce").await;
    require_certificate_rejection(result, CertificateRejection::UnknownIssuer, "untrusted CA")?;

    let local_address = SocketAddr::from(([127, 0, 0, 1], port));
    let wrong_name = client(ca_pem, Some(("wrong.invalid", local_address)))?;
    let result = request(&wrong_name, "wrong.invalid", port, "/nonce").await;
    require_certificate_rejection(
        result,
        CertificateRejection::NameMismatch,
        "wrong DNS identity",
    )?;

    let result = request(tls_client, "127.0.0.1", port, "/nonce").await;
    require_certificate_rejection(result, CertificateRejection::NameMismatch, "IP identity")?;

    let (status, body) = get_text(tls_client, host, port, "/nonce").await?;
    ensure!(
        status == 200 && body == nonce,
        "TLS endpoint changed after certificate rejections"
    );
    Ok(())
}

pub fn require_certificate_rejection(
    result: std::result::Result<Response, reqwest::Error>,
    expected: CertificateRejection,
    label: &str,
) -> Result<()> {
    let Err(error) = result else {
        bail!("{label} unexpectedly completed an HTTPS response");
    };
    ensure!(
        error.is_connect(),
        "{label} failed outside TLS connection setup"
    );
    ensure!(
        !error.is_timeout(),
        "{label} timed out instead of rejecting a certificate"
    );
    ensure!(
        !error.is_dns(),
        "{label} failed during DNS lookup instead of TLS verification"
    );
    ensure!(
        source_has_certificate_error(&error, expected),
        "{label} did not expose the expected rustls certificate-validation error: {error:?}"
    );
    Ok(())
}

fn source_has_certificate_error(error: &reqwest::Error, expected: CertificateRejection) -> bool {
    error_chain_has_certificate_error(error, expected)
}

fn error_chain_has_certificate_error(
    error: &(dyn StdError + 'static),
    expected: CertificateRejection,
) -> bool {
    let mut current = Some(error);
    for _ in 0..MAX_ERROR_CHAIN_DEPTH {
        let Some(error) = current else {
            return false;
        };
        if matches_rustls_certificate_error(error, expected) {
            return true;
        }
        current = error
            .downcast_ref::<std::io::Error>()
            .and_then(std::io::Error::get_ref)
            .map(|inner| inner as &(dyn StdError + 'static))
            .or_else(|| error.source());
    }
    false
}

fn matches_rustls_certificate_error(
    error: &(dyn StdError + 'static),
    expected: CertificateRejection,
) -> bool {
    let Some(RustlsError::InvalidCertificate(certificate_error)) =
        error.downcast_ref::<RustlsError>()
    else {
        return false;
    };
    match expected {
        CertificateRejection::UnknownIssuer => {
            matches!(certificate_error, CertificateError::UnknownIssuer)
        }
        CertificateRejection::NameMismatch => matches!(
            certificate_error,
            CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. }
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use rustls::{CertificateError, Error as RustlsError};

    use super::{CertificateRejection, error_chain_has_certificate_error};

    fn nested_io_error(certificate_error: RustlsError) -> io::Error {
        io::Error::other(io::Error::new(
            io::ErrorKind::InvalidData,
            certificate_error,
        ))
    }

    #[test]
    fn certificate_rejection_walks_nested_io_and_matches_the_exact_kind() {
        let unknown_issuer = nested_io_error(RustlsError::InvalidCertificate(
            CertificateError::UnknownIssuer,
        ));
        assert!(error_chain_has_certificate_error(
            &unknown_issuer,
            CertificateRejection::UnknownIssuer
        ));
        assert!(!error_chain_has_certificate_error(
            &unknown_issuer,
            CertificateRejection::NameMismatch
        ));

        let unrelated = io::Error::other(io::Error::new(io::ErrorKind::InvalidData, "unrelated"));
        assert!(!error_chain_has_certificate_error(
            &unrelated,
            CertificateRejection::UnknownIssuer
        ));
    }
}
