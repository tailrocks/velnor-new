//! Closed dependency declarations for qualified archive decoding.

use crate::impl_repo_policy::p11_toml;

/// Reviewed qualified-archive decoders are exclusive to the IO owner.
/// Complete declarations fix versions, disable defaults, and close features.
pub(super) fn reviewed_archive_dependency(dir: &str, key: &str, line: &str) -> bool {
    if dir != "crates/services/velnor-actions-orchestrator" {
        return false;
    }
    let (version, features) = match key {
        "flate2" => ("=1.1.10", Some("[\"rust_backend\"]")),
        "lzma-rust2" => ("=0.21.0", Some("[\"std\",\"xz\"]")),
        "tar" => ("=0.4.46", None),
        "zip" => ("=8.6.0", Some("[\"deflate-flate2\"]")),
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
        "zip = { version = \"=8.6.0\", default-features = false, features = [\"deflate-flate2\"] }";
    let owner = "crates/services/velnor-actions-orchestrator";
    assert!(reviewed_archive_dependency(owner, "zip", line));
    for dir in ["crates/apps/velnor-actions-cli", "crates/adapters/velnor-actions-mise"] {
        assert!(!reviewed_archive_dependency(dir, "zip", line));
    }
    for altered in [
        line.replace("=8.6.0", "=8.6.1"),
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
