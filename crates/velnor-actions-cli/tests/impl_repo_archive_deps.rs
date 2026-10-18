//! Closed dependency declarations for qualified archive decoding.

use crate::impl_repo_policy::p11_toml;

/// Reviewed qualified-archive decoders are exclusive to the IO owner.
/// Complete declarations fix versions, disable defaults, and close features.
pub(crate) fn reviewed_archive_dependency(dir: &str, key: &str, line: &str) -> bool {
    if dir != "crates/velnor-actions-orchestrator" {
        return false;
    }
    let (version, features) = match key {
        "flate2" => ("=1.1.10", Some("[\"rust_backend\"]")),
        "lzma-rust2" => ("=0.23.0", Some("[\"std\",\"xz\"]")),
        "tar" => ("=0.4.46", None),
        "zip" => ("=9.0.3", Some("[\"deflate-flate2\"]")),
        _ => return false,
    };
    let Some((_, value)) = line.split_once('=') else {
        return false;
    };
    let fields = p11_toml::inline_pairs(value);
    let expected_len = if features.is_some() { 3 } else { 2 };
    if fields.len() != expected_len {
        return false;
    }
    let find = |key| {
        fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    };
    if find("version") != Some(version) || find("default-features") != Some("false") {
        return false;
    }
    match (features, find("features")) {
        (None, None) => true,
        (Some(expected), Some(actual)) => {
            actual
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect::<String>()
                == expected
        }
        _ => false,
    }
}

#[test]
fn archive_dependency_policy_rejects_wrong_owner_pin_defaults_and_extra_features() {
    let line =
        "zip = { version = \"=9.0.3\", default-features = false, features = [\"deflate-flate2\"] }";
    let owner = "crates/velnor-actions-orchestrator";
    assert!(reviewed_archive_dependency(owner, "zip", line));
    for dir in ["crates/velnor-actions-cli", "crates/velnor-actions-mise"] {
        assert!(!reviewed_archive_dependency(dir, "zip", line));
    }
    for altered in [
        line.replace("=9.0.3", "=9.0.4"),
        line.replace("false", "true"),
        line.replace(
            "[\"deflate-flate2\"]",
            "[\"deflate-flate2\", \"aes-crypto\"]",
        ),
        line.replace(" }", ", optional = true }"),
    ] {
        assert!(!reviewed_archive_dependency(owner, "zip", &altered));
    }
    assert!(!reviewed_archive_dependency(owner, "tokio", line));
}

#[test]
fn lzma_dependency_policy_accepts_only_the_qualified_release() {
    let owner = "crates/velnor-actions-orchestrator";
    let line = "lzma-rust2 = { version = \"=0.23.0\", default-features = false, features = [\"std\", \"xz\"] }";
    assert!(reviewed_archive_dependency(owner, "lzma-rust2", line));
    assert!(!reviewed_archive_dependency(
        owner,
        "lzma-rust2",
        &line.replace("=0.23.0", "=0.22.0")
    ));
    assert!(!reviewed_archive_dependency(
        owner,
        "lzma-rust2",
        &line.replace("default-features = false", "default-features = true")
    ));
    assert!(!reviewed_archive_dependency(
        owner,
        "lzma-rust2",
        &line.replace("\"xz\"]", "\"xz\", \"encoder\"]")
    ));
}
