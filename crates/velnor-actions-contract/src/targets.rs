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

/// Canonical repository identity every release manifest MUST carry.
pub const EXPECTED_REPOSITORY: &str = "tailrocks/velnor-new";

/// Asset host allowlist: exactly `github.com`, never anything else.
const ASSET_HOST: &str = "github.com";

/// Fixed release-asset path prefix under the host.
const ASSET_PREFIX: &str = "/tailrocks/velnor-new/releases/download/";

/// Validate one release-asset URL against its manifest version and target.
///
/// Bootstrap/release contract §2: the URL MUST be
/// `https://github.com/tailrocks/velnor-new/releases/download/<tag>/
/// <asset>` where `<asset>` is exactly [`asset_filename`] for this
/// version and target and `<tag>` is non-empty without a `latest`
/// segment. Shape-only `https://` checks would let a merged manifest
/// redirect the Acquire step at attacker infrastructure. Userinfo,
/// query, fragment, `$`, backtick, and whitespace all fail closed.
///
/// Residual (X1/X4): same-version seed rollback stays review-gated. The
/// binding proves the URL names this version's official asset, but an
/// attacker who replaces the committed seed bytes at the same version
/// (or re-publishes the tag upstream) is caught only by reviewer
/// comparison against the published release. Follow-ups (scoped, not
/// dropped): Sigstore/SLSA attestation verification, a
/// published-vs-committed comparison job, and CODEOWNERS on the
/// committed manifest (bootstrap-and-release-contract §2).
/// # Errors
pub fn check_release_artifact(
    url: &str,
    version: &str,
    target: &str,
    file: &str,
    key: &str,
) -> Result<(), crate::errors::ContractError> {
    let bad = || crate::errors::ContractError::config(file, key, "unexpected_artifact_url");
    if url.bytes().any(|b| {
        b.is_ascii_whitespace() || b.is_ascii_control() || matches!(b, b'$' | b'`' | b'?' | b'#')
    }) {
        return Err(bad());
    }
    let Some(rest) = url.strip_prefix("https://") else {
        return Err(bad());
    };
    let Some((host, path)) = rest.split_once('/') else {
        return Err(bad());
    };
    if host != ASSET_HOST {
        return Err(bad());
    }
    let path = format!("/{path}");
    let Some(trailer) = path.strip_prefix(ASSET_PREFIX) else {
        return Err(bad());
    };
    let Some((tag, asset)) = trailer.split_once('/') else {
        return Err(bad());
    };
    if tag.is_empty() || tag == "latest" || asset.contains('/') {
        return Err(bad());
    }
    if asset != asset_filename(version, target) {
        return Err(bad());
    }
    Ok(())
}
