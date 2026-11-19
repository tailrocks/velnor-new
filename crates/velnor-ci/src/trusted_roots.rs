// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use ureq::tls::{Certificate, PemItem, RootCerts, parse_pem};

use crate::error::CiError;

pub const BUNDLE_VARIABLE: &str = "SSL_CERT_FILE";

pub fn roots_from_pem(pem: &[u8]) -> Result<RootCerts, CiError> {
    let mut certificates: Vec<Certificate<'static>> = Vec::new();
    for item in parse_pem(pem) {
        let item = item.map_err(|e| CiError::new(format!("a certificate is damaged: {e}")))?;
        if let PemItem::Certificate(certificate) = item {
            certificates.push(certificate);
        }
    }
    if certificates.is_empty() {
        return Err(CiError::new("the bundle holds no certificate"));
    }
    Ok(RootCerts::new_with_certs(&certificates))
}

#[must_use]
pub fn configured_roots() -> RootCerts {
    let Some(path) = std::env::var_os(BUNDLE_VARIABLE).filter(|path| !path.is_empty()) else {
        return RootCerts::WebPki;
    };
    let loaded = std::fs::read(&path)
        .map_err(|e| CiError::new(format!("cannot read it: {e}")))
        .and_then(|pem| roots_from_pem(&pem));
    loaded.unwrap_or_else(|error| {
        eprintln!(
            "{}",
            crate::escape::stderr_line(&format!(
                "ignoring {BUNDLE_VARIABLE}={}: {error}",
                path.to_string_lossy()
            ))
        );
        RootCerts::WebPki
    })
}
