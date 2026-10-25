//! Seed-witness-only formula verification for the first normal tap tree.

use velnor_actions_contract::{
    digest_b3,
    homebrew_spec::{
        parse_generated_formula, render_homebrew_formula, validate_homebrew_projection,
    },
    package_release::PackageReleaseChannel,
};
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

pub(super) fn verify_seed_cutover_formula_bytes(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    channel: PackageReleaseChannel,
    bytes: &[u8],
    witness: &crate::homebrew::structural_cutover::PrivateVerifiedSeedOutputBase,
    deadline: CheckDeadline,
) -> Result<(Option<String>, Vec<u8>), OrchestratorError> {
    match channel {
        PackageReleaseChannel::Preview => {
            verify_current_preview_formula_bytes(authority, bytes, deadline)
        }
        PackageReleaseChannel::Stable => {
            let mut plan = authority.plan().clone();
            plan.channel = channel;
            let projection = validate_homebrew_projection(authority.spec(), &plan)
                .map_err(|_| internal("native_check_formula_projection"))?;
            let parsed = parse_generated_formula(authority.spec(), &projection, bytes)
                .map_err(|_| internal("native_check_formula_invalid"))?;
            if !parsed.is_disabled_stable_sentinel() {
                return Err(internal("native_check_seed_cutover_stable_not_disabled"));
            }
            super::verify_stable_sentinel(authority.spec(), &projection, bytes)?;
            if !crate::homebrew::structural_cutover::is_exact_legacy_stable_formula(
                authority, witness, deadline,
            )? {
                return Err(internal("native_check_seed_cutover_legacy_stable_mismatch"));
            }
            Ok((None, bytes.to_vec()))
        }
    }
}

/// Verify candidate bytes against the current public Preview selected by the
/// protected source authority, without consulting seed-base formula history.
pub(super) fn verify_current_preview_formula_bytes(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<(Option<String>, Vec<u8>), OrchestratorError> {
    let (receipt_digest, formula) = render_current_preview_formula(authority, deadline)?;
    if formula != bytes {
        return Err(internal("native_check_formula_release_mismatch"));
    }
    Ok((Some(receipt_digest), formula))
}

/// Resolve and render only the current preview selected by protected authority.
pub(crate) fn render_current_preview_formula(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    deadline: CheckDeadline,
) -> Result<(String, Vec<u8>), OrchestratorError> {
    let mut release_plan = authority.plan().clone();
    release_plan.channel = PackageReleaseChannel::Preview;
    let projection = validate_homebrew_projection(authority.spec(), &release_plan)
        .map_err(|_| internal("native_check_formula_projection"))?;
    let source_sha = authority.authority_sha().to_owned();
    let selection = crate::package_release_public::PackageReleasePublicSelection::new(
        PackageReleaseChannel::Preview,
        format!("preview-{source_sha}"),
        source_sha,
    )?;
    let policy =
        crate::package_release_public::PackageReleasePublicPolicy::from_trusted_source_authority(
            release_plan,
            authority.source_repository_id(),
            authority.authority_sha().to_owned(),
        )?;
    let release = crate::package_release_public::verify_public_package_release(
        &policy, &selection, deadline,
    )?;
    let literals = super::formula_release_literals(authority, &projection, &release)?;
    let formula = render_homebrew_formula(authority.spec(), &literals)
        .map_err(|_| internal("native_check_formula_render"))?;
    let receipt_digest = digest_b3(
        &release
            .receipt()
            .canonical_bytes()
            .map_err(|_| internal("native_check_release_receipt_encode"))?,
    );
    Ok((receipt_digest, formula))
}
