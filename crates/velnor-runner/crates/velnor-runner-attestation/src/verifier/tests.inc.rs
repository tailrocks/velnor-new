#[cfg(test)]
mod tests {
    use super::{
        BoundedTufTransport, ExpectedClaims, ExpectedSubject, InlineVerifyRequest,
        MAX_BUNDLE_BYTES, MAX_TUF_RESPONSE_BYTES, VERIFY_DEADLINE, is_supported_ci_oid,
        single_claim, verify_inline_checksum_target,
    };
    use base64::Engine;
    use std::collections::BTreeMap;
    use std::time::Instant;
    use url::Url;

    #[test]
    fn tuf_transport_rejects_untrusted_origins_and_url_components() {
        for value in [
            "http://tuf-repo-cdn.sigstore.dev/metadata/timestamp.json",
            "https://example.com/metadata/timestamp.json",
            "https://user@tuf-repo-cdn.sigstore.dev/metadata/timestamp.json",
            "https://tuf-repo-cdn.sigstore.dev:8443/metadata/timestamp.json",
            "https://tuf-repo-cdn.sigstore.dev/metadata/timestamp.json?redirect=elsewhere",
            "https://tuf-repo-cdn.sigstore.dev/metadata/timestamp.json#fragment",
        ] {
            let parsed = Url::parse(value).expect("test URL parses");
            assert!(!BoundedTufTransport::allowed_url(&parsed));
        }
        let allowed = Url::parse("https://tuf-repo-cdn.sigstore.dev/metadata/timestamp.json")
            .expect("test URL parses");
        assert!(BoundedTufTransport::allowed_url(&allowed));
    }

    #[test]
    fn tuf_transport_enforces_response_byte_limit_without_overflow() {
        assert!(BoundedTufTransport::response_size_allowed(MAX_TUF_RESPONSE_BYTES - 1, 1));
        assert!(!BoundedTufTransport::response_size_allowed(MAX_TUF_RESPONSE_BYTES, 1));
        assert!(!BoundedTufTransport::response_size_allowed(usize::MAX, 1));
    }

    #[test]
    fn missing_and_duplicate_required_claims_fail_closed() {
        assert!(single_claim(&BTreeMap::new(), "1.3.6.1.4.1.57264.1.13").is_err());
        let duplicate = BTreeMap::from([(
            "1.3.6.1.4.1.57264.1.13".to_owned(),
            vec!["commit-a".to_owned(), "commit-b".to_owned()],
        )]);
        assert!(single_claim(&duplicate, "1.3.6.1.4.1.57264.1.13").is_err());
    }

    #[test]
    fn unsupported_fulcio_ci_extension_fails_closed() {
        assert!(is_supported_ci_oid("1.3.6.1.4.1.57264.1.25").is_err());
        assert!(!is_supported_ci_oid("1.3.6.1.4.1.57264.2.1").expect("foreign OID is ignored"));
    }

    #[tokio::test]
    async fn inline_entrypoint_rejects_oversized_bundle_before_network_access() {
        let request = InlineVerifyRequest {
            schema: 1,
            bundle_base64: base64::engine::general_purpose::STANDARD
                .encode(vec![b' '; MAX_BUNDLE_BYTES + 1]),
            checksum_base64: String::new(),
            expected: ExpectedClaims {
                signer: String::new(),
                signer_digest: String::new(),
                source: String::new(),
                source_digest: String::new(),
                source_ref: String::new(),
                build_config: String::new(),
                build_config_digest: String::new(),
            },
            checksum_subject: ExpectedSubject { name: String::new(), digest: String::new() },
            target_subject: ExpectedSubject { name: String::new(), digest: String::new() },
        };
        let result = verify_inline_checksum_target(request, Instant::now() + VERIFY_DEADLINE).await;
        assert!(result.is_err_and(|error| error.to_string() == "verification bundle exceeds bound"));
    }
}
