//! Policy-identity unit tests: version, digest, and profile shape.
use super::*;

fn config(version: &str, sha256: &str) -> RustPolicyConfig {
    RustPolicyConfig {
        version: version.to_owned(),
        sha256: sha256.to_owned(),
        profile: RustPolicyProfile::RustStrictV1,
    }
}

#[test]
fn valid_policy_passes() {
    let policy = config("0.1.3", &"a".repeat(64));
    assert!(policy.validate("test").is_ok());
}

#[test]
fn version_must_be_exact_triple() {
    for bad in [
        "", "1", "1.2", "1.2.3.4", "v1.2.3", "1.2.x", "1..3", " 1.2.3",
    ] {
        let policy = config(bad, &"a".repeat(64));
        let err = policy.validate("test").expect_err("version must fail");
        assert!(err.to_string().contains("bad_version"), "{err}");
    }
}

#[test]
fn sha256_must_be_64_lowercase_hex() {
    for bad in ["", "abc", &"a".repeat(63), &"a".repeat(65)] {
        let policy = config("0.1.3", bad);
        let err = policy.validate("test").expect_err("digest must fail");
        assert!(err.to_string().contains("bad_sha256"), "{err}");
    }
    let policy = config("0.1.3", &"A".repeat(64));
    assert!(policy.validate("test").is_err());
    let policy = config("0.1.3", &"g".repeat(64));
    assert!(policy.validate("test").is_err());
}

#[test]
fn profile_file_name_is_stable() {
    assert_eq!(
        RustPolicyProfile::RustStrictV1.file_name(),
        "rust-strict-v1.yml"
    );
}

#[test]
fn profile_rejects_unknown_values() {
    let raw = r#"{"version":"0.1.3","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","profile":"lax-v9"}"#;
    assert!(serde_json::from_str::<RustPolicyConfig>(raw).is_err());
}
