// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use ureq::tls::RootCerts;
use velnor_ci::error::CiError;
use velnor_ci::trusted_roots::roots_from_pem;

const CA: &str = "-----BEGIN CERTIFICATE-----\nMIIBZzCCAQ6gAwIBAgIUJGU7UxWHwQcSuuOAavq4qnCAs9IwCgYIKoZIzj0EAwIw\nGTEXMBUGA1UEAwwOdmVsbm9yLXRlc3QtY2EwIBcNMjYxMTE5MTEyODU2WhgPMjEy\nNjEwMjYxMTI4NTZaMBkxFzAVBgNVBAMMDnZlbG5vci10ZXN0LWNhMFkwEwYHKoZI\nzj0CAQYIKoZIzj0DAQcDQgAEQFpiTpKbmYkYyI+gsBMeTwFYeJGC3ZFf/U3mXczn\nlEP9bGFxMcBqxNNiMs/zL2uU3cSdyQtUiSnmvYwW+Ns4HKMyMDAwHQYDVR0OBBYE\nFNLJmmaLPYJcKvylI1E57IOCQoqbMA8GA1UdEwEB/wQFMAMBAf8wCgYIKoZIzj0E\nAwIDRwAwRAIgfLTyvNdWoP8nJsmJZRAhMwr79en2CV0ljTG6hQ/KTzUCIHymW5K3\nPycaTG6tKkdc91sPTYO+Yc1E3jKy8tL6qS/h\n-----END CERTIFICATE-----\n";

fn count(roots: RootCerts) -> usize {
    match roots {
        RootCerts::Specific(certs) => certs.len(),
        _ => 0,
    }
}

#[test]
fn a_bundle_of_runner_certificates_becomes_the_only_trusted_roots() -> Result<(), CiError> {
    let bundle = format!("{CA}{CA}");
    assert_eq!(count(roots_from_pem(bundle.as_bytes())?), 2);
    Ok(())
}

#[test]
fn a_bundle_without_a_certificate_is_refused() {
    let message = roots_from_pem(
        b"not a certificate
",
    )
    .err()
    .map_or_default(|e| e.to_string());
    assert!(message.contains("no certificate"), "{message}");
}

#[test]
fn a_damaged_certificate_is_refused() {
    let damaged = CA.replace("MIIB", "!!!!");
    assert!(roots_from_pem(damaged.as_bytes()).is_err());
}
