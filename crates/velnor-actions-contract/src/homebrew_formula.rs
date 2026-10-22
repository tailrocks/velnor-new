//! Literal-only Homebrew formula promotion contract.

use crate::{
    ContractError,
    package_release::{
        PackageReleaseAsset, PackageReleaseAssetKind, PackageReleaseChannel, PackageReleaseReceipt,
        PackageReleaseVersion,
    },
    package_release_plan::PackageReleasePlan,
    preview_identity::PreviewIdentity,
};
mod helpers;
use helpers::{
    FormulaSlots, is_full_sha, is_lower_hex, is_sha256, parse_metadata_prefix, parse_release_url,
    parse_sha256, parse_source_sha, parse_url, parse_version, safe_formula_path, spaces,
};

const MAX_FORMULA_BYTES: usize = 1024 * 1024;
/// Release identity authenticated by channel-specific publication verification,
/// signature checks, and attestations.
///
/// Construct this value only after verifying the published channel state, receipt
/// bundle, signer workflow, source authority, and each artifact attestation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPackageRelease {
    /// Canonical receipt digest verified by the caller.
    pub receipt_digest: String,
    /// Canonical, semantically valid receipt bytes parsed by Velnor.
    pub receipt: PackageReleaseReceipt,
}

impl VerifiedPackageRelease {
    /// Validate the receipt binding against the authenticated static release plan.
    ///
    /// # Errors
    ///
    /// Returns an identity error when channel identity, digest, or inventory differs.
    pub fn validate(
        &self,
        source_repository: &str,
        plan: &PackageReleasePlan,
    ) -> Result<(), ContractError> {
        self.receipt.validate_against_plan(plan)?;
        let source = &self.receipt.source.commit_sha;
        let channel_identity_matches = match self.receipt.channel {
            PackageReleaseChannel::Preview => {
                let expected = PreviewIdentity::derive(
                    &self.receipt.version.base,
                    source,
                    self.receipt.version.commit_count.unwrap_or(0),
                )?;
                self.receipt.version.value == expected.version
                    && self.receipt.version.source_sha7.as_deref() == Some(&source[..7])
                    && self.receipt.tag == expected.tag
            }
            PackageReleaseChannel::Stable => {
                self.receipt.version.value == self.receipt.version.base
                    && self.receipt.version.commit_count.is_none()
                    && self.receipt.version.source_sha7.is_none()
                    && self.receipt.tag == format!("v{}", self.receipt.version.base)
            }
        };
        if !channel_identity_matches
            || self.receipt.repository.slug != source_repository
            || plan.repository != source_repository
            || self.receipt.canonical_digest()? != self.receipt_digest
        {
            return Err(invalid("untrusted_release_receipt_identity"));
        }
        Ok(())
    }

    fn source_sha(&self) -> &str {
        &self.receipt.source.commit_sha
    }

    fn archive(&self, name: &str) -> Option<&PackageReleaseAsset> {
        self.receipt
            .assets
            .iter()
            .find(|asset| asset.name == name && asset.kind == PackageReleaseAssetKind::Payload)
    }
}

/// Captured tap-main formula and updater output, bound to one verified release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomebrewFormulaCandidate<'a> {
    /// Source repository slug from trusted Velnor configuration.
    pub source_repository: &'a str,
    /// Static product and target policy authenticated by the release signer.
    pub plan: &'a PackageReleasePlan,
    /// Destination tap slug from trusted Velnor configuration.
    pub tap_repository: &'a str,
    /// Exact configured formula path.
    pub formula_path: &'a str,
    /// Captured tap-main commit SHA.
    pub captured_base_sha: &'a str,
    /// Formula bytes read from that captured base.
    pub base_bytes: &'a [u8],
    /// Formula bytes proposed by the unprivileged updater.
    pub proposed_bytes: &'a [u8],
    /// Authenticated channel receipt identity.
    pub release: &'a VerifiedPackageRelease,
}

/// Derive canonical formula bytes and require exact equality with the proposal.
///
/// This function does not execute Ruby and does not validate Git tree metadata;
/// the caller must separately prove path/mode/tree identity before writing.
///
/// # Errors
///
/// Rejects malformed identity, unsupported formula syntax, downgrade or
/// conflicting retry, and any noncanonical or executable-code change.
pub fn validate_homebrew_formula_candidate(
    candidate: &HomebrewFormulaCandidate<'_>,
) -> Result<Vec<u8>, ContractError> {
    validate_candidate(candidate)?;
    let expected = rewrite_formula(
        candidate.base_bytes,
        candidate.source_repository,
        candidate.release,
    )?;
    if expected != candidate.proposed_bytes {
        return Err(invalid("formula_diff_is_not_literal_metadata_only"));
    }
    Ok(expected)
}

fn validate_candidate(candidate: &HomebrewFormulaCandidate<'_>) -> Result<(), ContractError> {
    if !crate::vcs::is_safe_github_repository(candidate.source_repository)
        || !crate::vcs::is_safe_github_repository(candidate.tap_repository)
        || !safe_formula_path(candidate.formula_path)
        || !is_full_sha(candidate.captured_base_sha)
    {
        return Err(invalid("invalid_captured_tap_base"));
    }
    candidate
        .release
        .validate(candidate.source_repository, candidate.plan)
}

fn rewrite_formula(
    base: &[u8],
    repository: &str,
    release: &VerifiedPackageRelease,
) -> Result<Vec<u8>, ContractError> {
    if base.is_empty() || base.len() > MAX_FORMULA_BYTES || !base.is_ascii() {
        return Err(invalid("formula_base_size_or_encoding"));
    }
    let text = std::str::from_utf8(base).map_err(|_| invalid("formula_base_encoding"))?;
    let mut out = String::with_capacity(text.len());
    let mut state = FormulaRewriteState::default();
    for line in text.split_inclusive('\n') {
        out.push_str(&state.rewrite_line(line, repository, release)?);
    }
    state.validate(release)?;
    Ok(out.into_bytes())
}

#[derive(Default)]
struct FormulaRewriteState {
    slots: FormulaSlots,
    prior_source: Option<String>,
    prior_version: Option<String>,
    last_url: Option<String>,
    seen_archives: Vec<String>,
}

impl FormulaRewriteState {
    fn rewrite_line(
        &mut self,
        line: &str,
        repository: &str,
        release: &VerifiedPackageRelease,
    ) -> Result<String, ContractError> {
        if let Some(source) = parse_source_sha(line) {
            self.slots.source += 1;
            if !is_full_sha(source) {
                return Err(invalid("formula_source_sha_malformed"));
            }
            self.prior_source = Some(source.to_owned());
            return Ok(format!("# source-sha: {}\n", release.source_sha()));
        }
        if let Some(version) = parse_version(line) {
            self.slots.version += 1;
            validate_old_version(version, release.receipt.channel)?;
            self.prior_version = Some(version.to_owned());
            return Ok(format!("  version \"{}\"\n", release.receipt.version.value));
        }
        if let Some(url) = parse_url(line) {
            return self.rewrite_url(line, url, repository, release);
        }
        if let Some(hash) = parse_sha256(line) {
            return self.rewrite_hash(line, hash, release);
        }
        if parse_metadata_prefix(line) {
            return Err(invalid("formula_metadata_syntax_unsupported"));
        }
        Ok(line.to_owned())
    }

    fn rewrite_url(
        &mut self,
        line: &str,
        url: &str,
        repository: &str,
        release: &VerifiedPackageRelease,
    ) -> Result<String, ContractError> {
        self.slots.urls += 1;
        let (tag, name) = parse_release_url(url, repository)?;
        let old_version = self
            .prior_version
            .as_deref()
            .ok_or_else(|| invalid("formula_version_must_precede_urls"))?;
        let expected_old_tag = match release.receipt.channel {
            PackageReleaseChannel::Preview => format!(
                "preview-{}",
                self.prior_source
                    .as_deref()
                    .ok_or_else(|| invalid("formula_source_sha_missing"))?
            ),
            PackageReleaseChannel::Stable => format!("v{old_version}"),
        };
        if tag != expected_old_tag || release.archive(name).is_none() {
            return Err(invalid("formula_base_release_binding_mismatch"));
        }
        if !self.seen_archives.iter().any(|seen| seen == name) {
            self.seen_archives.push(name.to_owned());
        }
        self.last_url = Some(name.to_owned());
        Ok(format!(
            "{}url \"https://github.com/{repository}/releases/download/{}/{name}\"\n",
            spaces(line),
            release.receipt.tag
        ))
    }

    fn rewrite_hash(
        &mut self,
        line: &str,
        hash: &str,
        release: &VerifiedPackageRelease,
    ) -> Result<String, ContractError> {
        self.slots.hashes += 1;
        if !is_sha256(hash) {
            return Err(invalid("formula_sha256_malformed"));
        }
        let name = self
            .last_url
            .take()
            .ok_or_else(|| invalid("formula_sha_without_adjacent_url"))?;
        let asset = release
            .archive(&name)
            .ok_or_else(|| invalid("formula_archive_not_in_receipt"))?;
        if self.prior_version.as_deref() == Some(release.receipt.version.value.as_str())
            && self.prior_source.as_deref() == Some(release.source_sha())
            && hash != asset.sha256
        {
            return Err(invalid("formula_preview_identity_conflict"));
        }
        Ok(format!("{}sha256 \"{}\"\n", spaces(line), asset.sha256))
    }

    fn validate(&self, release: &VerifiedPackageRelease) -> Result<(), ContractError> {
        let count = self.seen_archives.len();
        if self.slots
            != (FormulaSlots {
                source: 1,
                version: 1,
                urls: count,
                hashes: count,
            })
            || count == 0
            || self.last_url.is_some()
        {
            return Err(invalid("formula_metadata_slot_count_or_source_mismatch"));
        }
        let version = self
            .prior_version
            .as_deref()
            .ok_or_else(|| invalid("formula_version_missing"))?;
        let source = self
            .prior_source
            .as_deref()
            .ok_or_else(|| invalid("formula_source_sha_missing"))?;
        if release.receipt.channel == PackageReleaseChannel::Preview {
            let sha7 = version
                .rsplit_once('+')
                .map(|(_, sha)| sha)
                .ok_or_else(|| invalid("formula_version_not_canonical"))?;
            if !source.starts_with(sha7) {
                return Err(invalid("formula_source_version_mismatch"));
            }
        }
        validate_upgrade(
            version,
            source,
            &release.receipt.version,
            release.source_sha(),
            release.receipt.channel,
        )
    }
}

fn validate_old_version(value: &str, channel: PackageReleaseChannel) -> Result<(), ContractError> {
    if channel == PackageReleaseChannel::Stable {
        return if crate::config::is_exact_semver(value) {
            Ok(())
        } else {
            Err(invalid("formula_version_not_canonical"))
        };
    }
    let (base, suffix) = value
        .split_once("-preview.")
        .ok_or_else(|| invalid("formula_version_not_preview"))?;
    let (count, sha) = suffix
        .split_once('+')
        .ok_or_else(|| invalid("formula_version_not_canonical"))?;
    if !crate::config::is_exact_semver(base)
        || count.is_empty()
        || count.starts_with('0')
        || !count.bytes().all(|byte| byte.is_ascii_digit())
        || sha.len() != 7
        || !sha.bytes().all(is_lower_hex)
    {
        return Err(invalid("formula_version_not_canonical"));
    }
    Ok(())
}

fn validate_upgrade(
    old: &str,
    old_source: &str,
    next: &PackageReleaseVersion,
    source_sha: &str,
    channel: PackageReleaseChannel,
) -> Result<(), ContractError> {
    if channel == PackageReleaseChannel::Stable {
        let old_version = semver_parts(old)?;
        let next_version = semver_parts(&next.value)?;
        if next_version < old_version {
            return Err(invalid("formula_stable_downgrade"));
        }
        if next_version == old_version && old_source != source_sha {
            return Err(invalid("formula_stable_identity_conflict"));
        }
        return Ok(());
    }
    let (old_base, old_suffix) = old
        .split_once("-preview.")
        .ok_or_else(|| invalid("formula_version_not_preview"))?;
    let (old_count, old_sha7) = old_suffix
        .split_once('+')
        .ok_or_else(|| invalid("formula_version_not_canonical"))?;
    let old_count: u64 = old_count
        .parse()
        .map_err(|_| invalid("formula_version_not_canonical"))?;
    let old_base = semver_parts(old_base)?;
    let new_base = semver_parts(&next.base)?;
    if new_base < old_base || (new_base == old_base && next.commit_count.unwrap_or(0) < old_count) {
        return Err(invalid("formula_preview_downgrade"));
    }
    if new_base == old_base
        && next.commit_count == Some(old_count)
        && (old_sha7 != &source_sha[..7] || old_source != source_sha)
    {
        return Err(invalid("formula_preview_identity_conflict"));
    }
    Ok(())
}

fn semver_parts(value: &str) -> Result<(u64, u64, u64), ContractError> {
    if !crate::config::is_exact_semver(value) {
        return Err(invalid("formula_base_semver_invalid"));
    }
    let mut parts = value.split('.').map(|part| {
        part.parse::<u64>()
            .map_err(|_| invalid("formula_base_semver_invalid"))
    });
    Ok((
        parts
            .next()
            .ok_or_else(|| invalid("formula_base_semver_invalid"))??,
        parts
            .next()
            .ok_or_else(|| invalid("formula_base_semver_invalid"))??,
        parts
            .next()
            .ok_or_else(|| invalid("formula_base_semver_invalid"))??,
    ))
}

fn invalid(reason: &'static str) -> ContractError {
    ContractError::identity("homebrew_formula", reason)
}

#[cfg(test)]
#[path = "homebrew_formula_tests.rs"]
mod tests;
