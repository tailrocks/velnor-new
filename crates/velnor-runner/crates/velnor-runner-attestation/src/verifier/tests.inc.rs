#[cfg(test)]
mod tests {
    use super::{
        BoundedTufTransport, ExpectedClaims, ExpectedSubject, InlineVerifyRequest,
        MAX_BUNDLE_BYTES, MAX_TUF_RESPONSE_BYTES, VERIFY_DEADLINE, is_supported_ci_oid,
        single_claim, validate_expected_inputs, verify_inline_checksum_target,
        validate_state_directory,
    };
    use base64::Engine;
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
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

    #[test]
    fn fulcio_commit_claims_use_40_hex_while_artifact_digests_use_64() {
        let expected = ExpectedClaims {
            signer: "https://github.com/example/repo/.github/workflows/build.yml@refs/heads/main"
                .to_owned(),
            signer_digest: "a".repeat(40),
            source: "https://github.com/example/repo".to_owned(),
            source_digest: "b".repeat(40),
            source_ref: "refs/heads/main".to_owned(),
            build_config: "https://github.com/example/repo/.github/workflows/build.yml@refs/heads/main"
                .to_owned(),
            build_config_digest: "c".repeat(40),
        };
        let checksum = ExpectedSubject {
            name: "SHA256SUMS".to_owned(),
            digest: "d".repeat(64),
        };
        let target = ExpectedSubject {
            name: "velnor-host".to_owned(),
            digest: "e".repeat(64),
        };
        assert!(validate_expected_inputs(&expected, &checksum, &target).is_ok());

        for malformed in [
            "a".repeat(39),
            "A".repeat(40),
            "g".repeat(40),
            "a".repeat(64),
        ] {
            for field in 0..3 {
                let mut malformed_expected = expected.clone();
                match field {
                    0 => malformed_expected.signer_digest.clone_from(&malformed),
                    1 => malformed_expected.source_digest.clone_from(&malformed),
                    _ => malformed_expected.build_config_digest.clone_from(&malformed),
                }
                assert!(validate_expected_inputs(&malformed_expected, &checksum, &target).is_err());
            }
        }

        let mut malformed_checksum = checksum.clone();
        malformed_checksum.digest = "d".repeat(40);
        assert!(validate_expected_inputs(&expected, &malformed_checksum, &target).is_err());

        malformed_checksum.digest = "g".repeat(64);
        assert!(validate_expected_inputs(&expected, &malformed_checksum, &target).is_err());

        let mut malformed_target = target.clone();
        malformed_target.digest = "e".repeat(65);
        assert!(validate_expected_inputs(&expected, &checksum, &malformed_target).is_err());
}
    #[tokio::test]
    async fn inline_entrypoint_rejects_oversized_bundle_before_network_access() {
        let state = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .expect("create private service state");
        let state_directory = fs::canonicalize(state.path())
            .expect("canonicalize service state")
            .to_string_lossy()
            .into_owned();
        let request = InlineVerifyRequest {
            schema: 2,
            state_directory,
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

    #[tokio::test]
    async fn malformed_state_root_is_rejected_before_payload_processing() {
        let request = InlineVerifyRequest {
            schema: 2,
            state_directory: "relative/state".to_owned(),
            bundle_base64: "!".to_owned(),
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
            checksum_subject: ExpectedSubject {
                name: String::new(),
                digest: String::new(),
            },
            target_subject: ExpectedSubject {
                name: String::new(),
                digest: String::new(),
            },
        };
        let error = verify_inline_checksum_target(request, Instant::now() + VERIFY_DEADLINE)
            .await
            .expect_err("relative service state must fail closed");
        assert_eq!(
            error.to_string(),
            "state directory is malformed or exceeds byte limit"
        );
    }

    #[test]
    fn state_root_requires_canonical_private_existing_directory() {
        let parent = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .expect("create private test parent");
        let state = parent.path().join("state");
        fs::create_dir(&state).expect("create service state");
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700))
            .expect("make service state private");
        let canonical = fs::canonicalize(&state).expect("canonicalize service state");
        let value = canonical.to_str().expect("test path is UTF-8");
        assert_eq!(
            validate_state_directory(value).expect("private canonical root is accepted"),
            canonical
        );
        assert!(validate_state_directory("relative/state").is_err());
        assert!(validate_state_directory(&format!("{value}/../state")).is_err());
        assert!(validate_state_directory(&format!("{value}/./child")).is_err());
        assert!(validate_state_directory(&format!("/{0}", "s".repeat(4096))).is_err());

        let shared = parent.path().join("shared");
        fs::create_dir(&shared).expect("create unsafe state root");
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o777))
            .expect("make state root shared");
        assert!(validate_state_directory(shared.to_str().expect("test path is UTF-8")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn state_root_rejects_symlinked_components() {
        use std::os::unix::fs::symlink;

        let parent = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .expect("create private test parent");
        let state = parent.path().join("state");
        fs::create_dir(&state).expect("create service state");
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700))
            .expect("make service state private");
        let alias = parent.path().join("alias");
        symlink(&state, &alias).expect("create symlinked state alias");
        assert!(validate_state_directory(alias.to_str().expect("test path is UTF-8")).is_err());
    }

}
