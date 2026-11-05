// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;
use std::time::Duration;

use velnor_actions_mise::CheckDeadline;

use super::{agent, checked_storage_uri, identity_encoding, storage_request_headers};
use crate::{OrchestratorError, internal::internal};

pub(crate) fn fetch_attestation_bundle(
    url: &str,
    limit: usize,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let remaining = deadline
        .remaining()
        .map_err(|_| internal("package_verification_deadline"))?;
    let response = storage_request(agent()?, url, remaining.min(Duration::from_secs(90)))?
        .call()
        .map_err(|_| internal("package_attestation_bundle_transport"))?;
    if response.status().as_u16() != 200 || !identity_encoding(response.headers()) {
        return Err(internal("package_attestation_bundle_status"));
    }
    let mut compressed = Vec::new();
    response
        .into_body()
        .into_reader()
        .take((limit as u64).saturating_add(1))
        .read_to_end(&mut compressed)
        .map_err(|_| internal("package_attestation_bundle_read"))?;
    if deadline.remaining().is_err() {
        return Err(internal("package_verification_deadline"));
    }
    decode_bundle(&compressed, limit)
}

fn storage_request(
    agent: &ureq::Agent,
    url: &str,
    timeout: Duration,
) -> Result<ureq::RequestBuilder<ureq::typestate::WithoutBody>, OrchestratorError> {
    let url = checked_storage_uri(url)?;
    let mut request = agent.get(&url);
    for (name, value) in storage_request_headers() {
        request = request.header(name, &value);
    }
    Ok(request
        .config()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .build())
}

fn decode_bundle(compressed: &[u8], limit: usize) -> Result<Vec<u8>, OrchestratorError> {
    if compressed.len() > limit {
        return Err(internal("package_attestation_bundle_limit"));
    }
    let size = snap::raw::decompress_len(compressed)
        .map_err(|_| internal("package_attestation_bundle_invalid"))?;
    if size == 0 || size > limit {
        return Err(internal("package_attestation_bundle_limit"));
    }
    snap::raw::Decoder::new()
        .decompress_vec(compressed)
        .map_err(|_| internal("package_attestation_bundle_invalid"))
}

#[cfg(test)]
mod tests {
    use super::{decode_bundle, identity_encoding, storage_request};
    use std::time::Duration;

    #[test]
    fn decodes_bounded_signed_bundle_and_rejects_invalid_data() {
        let bytes = br#"{"mediaType":"application/vnd.dev.sigstore.bundle+json"}"#;
        let compressed = snap::raw::Encoder::new()
            .compress_vec(bytes)
            .expect("compress fixture");
        assert_eq!(
            decode_bundle(&compressed, 1024).expect("decode bundle"),
            bytes
        );
        assert!(decode_bundle(&compressed, compressed.len() - 1).is_err());
        let expanded = format!("{{\"payload\":\"{}\"}}", "A".repeat(4096));
        let compact = snap::raw::Encoder::new()
            .compress_vec(expanded.as_bytes())
            .expect("compress expanded fixture");
        let limit = 1024;
        assert!(compact.len() < limit && expanded.len() > limit);
        assert!(decode_bundle(&compact, limit).is_err());
        assert!(decode_bundle(&compressed[..compressed.len() - 1], 1024).is_err());
        assert!(decode_bundle(&[0], 1024).is_err());
        assert!(decode_bundle(&[], 1024).is_err());
    }

    #[test]
    fn storage_requests_restrict_origin_and_omit_github_credentials() {
        let agent = ureq::Agent::new_with_defaults();
        let request = storage_request(
            &agent,
            "https://fixture.blob.core.windows.net/bundle?sig=fixture",
            Duration::from_secs(1),
        )
        .expect("allowed storage URL");
        let headers = request.headers_ref().expect("request headers");
        assert_eq!(
            headers
                .get("accept-encoding")
                .and_then(|value| value.to_str().ok()),
            Some("identity")
        );
        assert!(!headers.contains_key("authorization"));
        assert!(!headers.contains_key("gh-token"));
        for url in [
            "http://fixture.blob.core.windows.net/bundle",
            "https://user@fixture.blob.core.windows.net/bundle",
            "https://example.com/bundle",
        ] {
            assert!(storage_request(&agent, url, Duration::from_secs(1)).is_err());
        }
        let mut response_headers = ureq::http::HeaderMap::new();
        assert!(identity_encoding(&response_headers));
        response_headers.insert("content-encoding", "gzip".parse().expect("header"));
        assert!(!identity_encoding(&response_headers));
    }
}
