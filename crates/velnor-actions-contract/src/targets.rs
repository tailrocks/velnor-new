//! Supported release targets and release-asset naming.
//!
//! Bootstrap/release contract §2-§3: one immutable asset per target plus a
//! versioned release manifest. Runner labels map to the single Linux target;
//! macOS targets exist for local release installs only.

/// Every supported release target triple, in manifest order.
pub const SUPPORTED_TARGETS: [&str; 3] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
];

/// Release-manifest JSON asset filename (version is inside the JSON).
pub const RELEASE_MANIFEST_FILENAME: &str = "velnor-actions-release-manifest.json";

/// Whether `target` is a supported release triple.
#[must_use]
pub fn is_supported_target(target: &str) -> bool {
    SUPPORTED_TARGETS.contains(&target)
}

/// Release-asset filename for one version and target triple.
#[must_use]
pub fn asset_filename(version: &str, target: &str) -> String {
    format!("velnor-actions-{version}-{target}")
}

/// Runner-label to release-target mapping.
///
/// Versioned `ubuntu-*` x64 labels resolve to Linux x86-64. `-arm` labels
/// have no supported target yet and return `None` (consumer generation
/// fails with `unsupported_target_for_runner` rather than embedding a
/// wrong-architecture asset).
#[must_use]
pub fn target_for_runner_label(label: &str) -> Option<&'static str> {
    match label {
        "ubuntu-22.04" | "ubuntu-24.04" | "ubuntu-26.04" => Some(SUPPORTED_TARGETS[0]),
        _ => None,
    }
}
