//! Typed release targets and release-asset naming.
//!
//! Bootstrap/release contract §2-§3: one immutable asset per target plus a
//! versioned release manifest. Target IDs stay explicit at every boundary;
//! callers must not infer platform meaning from array positions.

/// One compiled and published Velnor release target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReleaseTarget {
    /// Linux x86-64 GNU ABI.
    LinuxX86_64,
    /// macOS arm64.
    MacosArm64,
    /// macOS x86-64.
    MacosX86_64,
}

impl ReleaseTarget {
    /// All supported targets in canonical manifest order.
    pub const ALL: [Self; 3] = [Self::LinuxX86_64, Self::MacosArm64, Self::MacosX86_64];

    /// Target triple used in release records and filenames.
    #[must_use]
    pub const fn triple(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "x86_64-unknown-linux-gnu",
            Self::MacosArm64 => "aarch64-apple-darwin",
            Self::MacosX86_64 => "x86_64-apple-darwin",
        }
    }

    /// Parse one exact supported target triple.
    #[must_use]
    pub fn parse_triple(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|target| target.triple() == value)
    }

    /// Resolve a GitHub runner label to its native release target.
    #[must_use]
    pub fn for_runner_label(label: &str) -> Option<Self> {
        match label {
            "ubuntu-22.04" | "ubuntu-24.04" | "ubuntu-26.04" => Some(Self::LinuxX86_64),
            "macos-14" | "macos-15" | "macos-15-arm64" | "macos-26" => Some(Self::MacosArm64),
            "macos-15-intel" | "macos-26-intel" => Some(Self::MacosX86_64),
            _ => None,
        }
    }
}

/// Supported release triples projected from the canonical typed target set.
pub const SUPPORTED_TARGETS: [&str; 3] = [
    ReleaseTarget::LinuxX86_64.triple(),
    ReleaseTarget::MacosArm64.triple(),
    ReleaseTarget::MacosX86_64.triple(),
];

/// Canonical release-manifest JSON asset filename (version is inside the JSON).
pub const RELEASE_MANIFEST_FILENAME: &str = "release-manifest.json";

/// Whether `target` is a supported release triple.
#[must_use]
pub fn is_supported_target(target: &str) -> bool {
    ReleaseTarget::parse_triple(target).is_some()
}

/// Release-asset filename for one version and target triple.
#[must_use]
pub fn asset_filename(version: &str, target: &str) -> String {
    format!("velnor-actions-{version}-{target}")
}

/// Canonical repository identity every release manifest MUST carry.
pub const EXPECTED_REPOSITORY: &str = "tailrocks/velnor-new";

/// Asset host allowlist: exactly `github.com`, never anything else.
const ASSET_HOST: &str = "github.com";

/// Fixed release-asset path prefix under the host.
const ASSET_PREFIX: &str = "tailrocks/velnor-new/releases/download/";

/// Validate a release-asset URL and return its final path segment.
///
/// # Errors
fn checked_release_asset_name<'a>(
    url: &'a str,
    version: &str,
    commit: &str,
    file: &str,
    key: &str,
) -> Result<&'a str, crate::errors::ContractError> {
    let bad = || crate::errors::ContractError::config(file, key, "unexpected_artifact_url");
    crate::manifest_checks::check_semver(version, file, "version")?;
    crate::manifest_checks::check_commit(commit, file, "commit")?;
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
    let Some(trailer) = path.strip_prefix(ASSET_PREFIX) else {
        return Err(bad());
    };
    // Split from the right: seed tags legitimately contain slashes.
    let Some((tag, asset)) = trailer.rsplit_once('/') else {
        return Err(bad());
    };
    if asset.is_empty() || !is_release_tag_for_source(tag, version, commit) {
        return Err(bad());
    }
    Ok(asset)
}

/// Accept only release tags whose spelling is bound to the manifest.
fn is_release_tag_for_source(tag: &str, version: &str, commit: &str) -> bool {
    tag.strip_prefix('v')
        .is_some_and(|tag_version| tag_version == version)
        || tag.strip_prefix("generator-").is_some_and(|tag_commit| {
            crate::ids::is_lower_hex_len(tag_commit, 40) && tag_commit == commit
        })
        || is_seed_tag_for_version(tag, version)
}

/// Validate one release-asset URL against its manifest version and target.
///
/// Bootstrap/release contract §2: the URL MUST be
/// `https://github.com/tailrocks/velnor-new/releases/download/<tag>/
/// <asset>` where `<asset>` is exactly [`asset_filename`] for this
/// version and target. `<tag>` MUST be `v<version>`,
/// `generator-<manifest commit>`, or a seed tag bound to this version
/// ([`is_seed_tag_for_version`]). Exact grammar rejects path traversal,
/// backslash, percent-encoding, and other URL-normalization ambiguity.
/// Userinfo, query, fragment, `$`, backtick, and whitespace also fail
/// closed.
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
    commit: &str,
    target: &str,
    file: &str,
    key: &str,
) -> Result<(), crate::errors::ContractError> {
    let asset = checked_release_asset_name(url, version, commit, file, key)?;
    if asset != asset_filename(version, target) {
        return Err(crate::errors::ContractError::config(
            file,
            key,
            "unexpected_artifact_url",
        ));
    }
    Ok(())
}

/// Validate the separately published release-manifest asset URL.
///
/// The JSON manifest records target assets but not its own URL. Callers
/// handling the release asset URL use this check to enforce the canonical
/// filename alongside the same host and release-path rules as binaries.
/// # Errors
pub fn check_release_manifest_artifact(
    url: &str,
    version: &str,
    commit: &str,
    file: &str,
    key: &str,
) -> Result<(), crate::errors::ContractError> {
    let asset = checked_release_asset_name(url, version, commit, file, key)?;
    if asset != RELEASE_MANIFEST_FILENAME {
        return Err(crate::errors::ContractError::config(
            file,
            key,
            "unexpected_artifact_url",
        ));
    }
    Ok(())
}

/// Seed tag prefix: immutable `seed/`-namespaced generator pre-releases.
const SEED_TAG_PREFIX: &str = "seed/velnor-actions-";

/// True for a seed tag bound to `version`.
///
/// Accepts exactly `seed/velnor-actions-<version>` or
/// `seed/velnor-actions-<version>-<N>` with a numeric counter (`-N`
/// distinguishes seed builds of one generator version). Seed tags are
/// published as immutable releases, but same-version substitution
/// stays review-gated. Both callers pass a
/// semver-checked `version`, so the literal match cannot smuggle path
/// metacharacters; anything else, including a version-mismatched seed
/// tag, fails closed.
#[must_use]
pub fn is_seed_tag_for_version(tag: &str, version: &str) -> bool {
    let Some(rest) = tag.strip_prefix(SEED_TAG_PREFIX) else {
        return false;
    };
    if rest == version {
        return true;
    }
    rest.strip_prefix(version).is_some_and(|suffix| {
        suffix.len() > 1
            && suffix.starts_with('-')
            && suffix[1..].bytes().all(|b| b.is_ascii_digit())
    })
}

/// Version marker for the contract schema shell.
pub const CONTRACT_VERSION: u32 = 0;
