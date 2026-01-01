//! Synthetic byte fixtures exercise IO bounds; these are not release qualification.
use super::*;

#[test]
fn each_platform_bound_matches_qualified_installed_bytes() {
    for (platform, sha256, bytes) in [
        (
            CheckPlatform::LinuxX64,
            MISE_BINARY_SHA256_LINUX_X64,
            158_829_568,
        ),
        (
            CheckPlatform::MacosArm64,
            MISE_BINARY_SHA256_MACOS_ARM64,
            125_641_616,
        ),
        (
            CheckPlatform::MacosX64,
            MISE_BINARY_SHA256_MACOS_X64,
            153_265_264,
        ),
    ] {
        let pin = mise_binary_pin(platform);
        assert_eq!(pin.sha256, sha256);
        assert_eq!(pin.max_bytes, bytes);
        assert!(pin.max_bytes > 64 * 1024 * 1024);
    }
}

#[test]
fn verified_bytes_above_old_budget_copy_identically() {
    let temp = tempfile::TempDir::new().expect("temp");
    let source = temp.path().join("synthetic-source");
    let destination = temp.path().join("owned");
    let bytes = vec![0x42; 64 * 1024 * 1024 + 1];
    let expected = crate::cover_identity::generator::sha256_hex(&bytes);
    std::fs::write(&source, &bytes).expect("synthetic bytes");
    assert_eq!(
        crate::retrieve_reports::read_staged_bytes(&source, 64 * 1024 * 1024),
        Err("oversize"),
        "old budget rejects otherwise verified bytes"
    );
    project_binary(
        &source,
        &destination,
        &expected,
        mise_binary_pin(CheckPlatform::MacosArm64).max_bytes,
    )
    .expect("qualified bound admits bytes");
    assert_eq!(std::fs::read(&destination).expect("owned bytes"), bytes);
}

#[test]
fn one_byte_over_each_qualified_bound_fails_before_digest_or_copy() {
    let temp = tempfile::TempDir::new().expect("temp");
    let source = temp.path().join("sparse-oversize-fixture");
    for platform in [
        CheckPlatform::LinuxX64,
        CheckPlatform::MacosArm64,
        CheckPlatform::MacosX64,
    ] {
        let pin = mise_binary_pin(platform);
        let file = std::fs::File::create(&source).expect("sparse fixture");
        file.set_len(pin.max_bytes + 1).expect("fixture length");
        let destination = temp.path().join(platform.target());
        let error = project_binary(&source, &destination, pin.sha256, pin.max_bytes)
            .expect_err("over qualified bound");
        assert!(
            error.to_string().contains("mise_binary_unreadable"),
            "{error}"
        );
        assert!(!error.to_string().contains("mise_binary_unqualified_sha256"));
        assert!(!destination.exists());
    }
}

#[test]
fn in_bound_wrong_digest_still_refuses_copy() {
    let temp = tempfile::TempDir::new().expect("temp");
    let source = temp.path().join("synthetic-source");
    let destination = temp.path().join("owned");
    std::fs::write(&source, b"unqualified synthetic bytes").expect("fixture");
    let pin = mise_binary_pin(CheckPlatform::LinuxX64);
    let error = project_binary(&source, &destination, pin.sha256, pin.max_bytes)
        .expect_err("wrong identity");
    assert!(error.to_string().contains("mise_binary_unqualified_sha256"));
    assert!(!destination.exists());
}
