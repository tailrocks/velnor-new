use super::activation::create_fresh_install_directories;
use super::support::{
    parse_brew_ruby_config, parse_portable_ruby_archive_sha256, validate_host_identity_values,
    validate_profile_evidence,
};
use super::{
    BREW_DESCRIBE, BREW_PINNED_COMMIT, BREW_PINNED_TREE, HostedHomebrewProfile,
    PORTABLE_RUBY_ARCHIVE_SHA256, PORTABLE_RUBY_VERSION, is_b3_digest, is_git_sha1,
};
use std::path::PathBuf;

#[test]
fn hosted_runner_gate_requires_github_macos_arm64() {
    assert!(
        validate_host_identity_values("true", "github-hosted", "macOS", "ARM64", "aarch64").is_ok()
    );
    assert!(
        validate_host_identity_values("false", "github-hosted", "macOS", "ARM64", "aarch64")
            .is_err()
    );
    assert!(
        validate_host_identity_values("true", "self-hosted", "macOS", "ARM64", "aarch64").is_err()
    );
    assert!(
        validate_host_identity_values("true", "github-hosted", "Linux", "ARM64", "aarch64")
            .is_err()
    );
    assert!(
        validate_host_identity_values("true", "github-hosted", "macOS", "X64", "x86_64").is_err()
    );
    assert!(
        validate_host_identity_values("true", "github-hosted", "macOS", "ARM64", "x86_64").is_err()
    );
}

#[test]
fn hosted_brew_profile_pins_source_tree_version_and_ruby_record_separately() {
    let profile = HostedHomebrewProfile {
        prefix: PathBuf::from("/opt/homebrew"),
        previous_prefix: PathBuf::from(
            "/Users/runner/work/_temp/velnor-homebrew-recovery-test/previous",
        ),
        previous_prefix_device: 1,
        previous_prefix_inode: 11,
        active_prefix_device: 1,
        active_prefix_inode: 12,
        source_commit: BREW_PINNED_COMMIT.to_owned(),
        source_tree: BREW_PINNED_TREE.to_owned(),
        source_describe: BREW_DESCRIBE.to_owned(),
        source_tag_commit: "8f10429b56be28a25032b87463fffc95e556b5bc".to_owned(),
        portable_ruby_version: PORTABLE_RUBY_VERSION.to_owned(),
        portable_ruby_path: PathBuf::from(
            "/opt/homebrew/Library/Homebrew/vendor/portable-ruby/current/bin/ruby",
        ),
        portable_ruby_archive_sha256: PORTABLE_RUBY_ARCHIVE_SHA256.to_owned(),
        portable_ruby_binary_sha256: "a".repeat(64),
    };
    assert!(validate_profile_evidence(&profile).is_ok());
    assert_ne!(
        profile.portable_ruby_archive_sha256,
        profile.portable_ruby_binary_sha256
    );

    let mut altered = profile.clone();
    altered.source_tree = "b".repeat(40);
    assert!(validate_profile_evidence(&altered).is_err());

    let mut altered = profile.clone();
    altered.portable_ruby_version = "4.0.8".to_owned();
    assert!(validate_profile_evidence(&altered).is_err());

    let mut altered = profile.clone();
    altered.portable_ruby_path =
        PathBuf::from("/opt/homebrew/Library/Homebrew/vendor/portable-ruby/4.0.8/bin/ruby");
    assert!(validate_profile_evidence(&altered).is_err());

    let mut altered = profile;
    altered.portable_ruby_binary_sha256 = "not-a-digest".to_owned();
    assert!(validate_profile_evidence(&altered).is_err());
}

#[test]
fn brew_config_must_report_one_absolute_portable_ruby_path_and_version() {
    let parsed = parse_brew_ruby_config(
        "HOMEBREW_PREFIX: /opt/homebrew\nHomebrew Ruby: 4.0.9 => /opt/homebrew/Library/Homebrew/vendor/portable-ruby/current/bin/ruby\n",
    )
    .expect("valid Homebrew runtime config");
    assert_eq!(parsed.0, "4.0.9");
    assert_eq!(
        parsed.1,
        PathBuf::from("/opt/homebrew/Library/Homebrew/vendor/portable-ruby/current/bin/ruby")
    );
    assert!(parse_brew_ruby_config("Homebrew Ruby: 4.0.8 => /opt/homebrew/ruby\n").is_some());
    assert!(parse_brew_ruby_config(
        "Homebrew Ruby: 4.0.9 => /opt/homebrew/ruby\nHomebrew Ruby: 4.0.9 => /opt/homebrew/ruby\n"
    )
    .is_none());
    assert!(parse_brew_ruby_config("Homebrew Ruby: 4.0.9 => relative/ruby\n").is_none());
    assert!(parse_brew_ruby_config("Homebrew Ruby: 4.0.9 /opt/homebrew/ruby\n").is_none());
    assert!(parse_brew_ruby_config("Homebrew Ruby: => /opt/homebrew/ruby\n").is_none());
}

#[test]
fn portable_ruby_archive_record_must_have_one_exact_sha256_assignment() {
    assert_eq!(
        parse_portable_ruby_archive_sha256(&format!(
            "# fixed source record\nruby_SHA={PORTABLE_RUBY_ARCHIVE_SHA256}\n"
        )),
        Some(PORTABLE_RUBY_ARCHIVE_SHA256)
    );
    assert!(parse_portable_ruby_archive_sha256("ruby_SHA=not-a-digest\n").is_none());
    assert!(
        parse_portable_ruby_archive_sha256(&format!(
            "ruby_SHA={PORTABLE_RUBY_ARCHIVE_SHA256}\nruby_SHA={PORTABLE_RUBY_ARCHIVE_SHA256}\n"
        ))
        .is_none()
    );
    assert!(parse_portable_ruby_archive_sha256("ruby_sha=abc\n").is_none());
}

#[test]
fn native_custody_uses_typed_blake3_and_git_sha1_grammars() {
    assert!(is_b3_digest(&format!("b3-{}", "a".repeat(64))));
    assert!(!is_b3_digest(&"a".repeat(64)));
    assert!(!is_b3_digest(&format!("b3-{}", "A".repeat(64))));
    assert!(is_git_sha1(&"b".repeat(40)));
    assert!(!is_git_sha1(&"b".repeat(64)));
    assert!(!is_git_sha1(&"G".repeat(40)));
}

#[test]
fn fresh_profile_creates_owned_cellar_and_opt_directories() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().expect("temporary prefix");
    create_fresh_install_directories(root.path()).expect("create install directories");
    for name in ["Cellar", "opt"] {
        let metadata = std::fs::symlink_metadata(root.path().join(name)).expect("directory");
        assert!(metadata.is_dir());
        assert!(!metadata.file_type().is_symlink());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o755);
    }
}

#[test]
fn fresh_profile_rejects_a_symlinked_install_directory() {
    let root = tempfile::tempdir().expect("temporary prefix");
    let target = root.path().join("target");
    std::fs::create_dir(&target).expect("target directory");
    std::os::unix::fs::symlink(&target, root.path().join("Cellar")).expect("Cellar symlink");
    assert!(create_fresh_install_directories(root.path()).is_err());
}
