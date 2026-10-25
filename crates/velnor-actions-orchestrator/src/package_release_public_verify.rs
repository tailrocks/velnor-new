use velnor_actions_contract::{
    PackageReleasePlan,
    package_release::{PackageReleaseChannel, PackageReleaseReceipt},
};
use velnor_actions_mise::CheckDeadline;

use super::{
    HistoricalPackageReleaseAdmission, HistoricalPackageReleaseRecovery, OrchestratorError,
    PUBLIC_VERIFY_BUDGET, PackageReleasePublicPolicy, PackageReleasePublicSelection,
    VerifiedPublicPackageRelease,
    assets::{
        is_full_sha, public_asset_size_limits, sha256, signing_identity, tag_for,
        verified_file_inventory, write_new_file,
    },
};
use crate::{
    internal::internal,
    package_release_publish::{PublicReleaseSnapshot, read_public_release_snapshot},
    package_release_signer::{
        VerifiedAttestedSignedPackage,
        verify_attested_signed_package_with_owned_tempdir_and_deadlines,
    },
};
pub(crate) fn verify_public_package_release(
    policy: &PackageReleasePublicPolicy,
    selection: &PackageReleasePublicSelection,
    deadline: CheckDeadline,
) -> Result<VerifiedPublicPackageRelease, OrchestratorError> {
    let remaining = deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    let public_deadline = CheckDeadline::after(remaining.min(PUBLIC_VERIFY_BUDGET))
        .map_err(|_| internal("package_public_verification_deadline"))?;
    deadline
        .scope(|| {
            public_deadline
                .scope(|| {
                    verify_public_package_release_inner(
                        policy,
                        selection,
                        public_deadline,
                        deadline,
                    )
                })
                .map_err(|_| internal("package_public_verification_deadline"))?
        })
        .map_err(|_| internal("package_public_verification_deadline"))?
}

fn verify_public_package_release_inner(
    policy: &PackageReleasePublicPolicy,
    selection: &PackageReleasePublicSelection,
    verification_deadline: CheckDeadline,
    custody_deadline: CheckDeadline,
) -> Result<VerifiedPublicPackageRelease, OrchestratorError> {
    let snapshot = read_verified_snapshot(policy, selection, verification_deadline)?;
    let authority =
        verify_public_receipt_authority(policy, selection, &snapshot, verification_deadline)?;
    let signed = verify_public_release_custody(
        policy,
        selection,
        &snapshot,
        &authority,
        verification_deadline,
        custody_deadline,
    )?;
    Ok(VerifiedPublicPackageRelease {
        plan: policy.plan.clone(),
        receipt: authority.receipt,
        signed,
        historical: authority.historical,
        authority_sha: policy.authority_sha.clone(),
    })
}

struct PublicReceiptAuthority {
    receipt: PackageReleaseReceipt,
    historical: bool,
    recovery: Option<HistoricalPackageReleaseRecovery>,
}

fn read_verified_snapshot(
    policy: &PackageReleasePublicPolicy,
    selection: &PackageReleasePublicSelection,
    deadline: CheckDeadline,
) -> Result<PublicReleaseSnapshot, OrchestratorError> {
    let plan = &policy.plan;
    if selection.channel != plan.channel
        || !is_full_sha(&selection.source_sha)
        || selection.tag != tag_for(plan, &selection.source_sha)?
    {
        return Err(internal("package_public_selection_policy_mismatch"));
    }
    let snapshot = read_public_release_snapshot(
        &plan.repository,
        policy.repository_id,
        &plan.default_branch,
        &selection.tag,
        &public_asset_size_limits(plan)?,
        deadline,
    )?;
    if snapshot.tag_source_sha != selection.source_sha
        || snapshot.draft
        || snapshot.prerelease != (selection.channel == PackageReleaseChannel::Preview)
        || !snapshot.immutable
    {
        return Err(internal("package_public_release_state_mismatch"));
    }
    Ok(snapshot)
}

fn verify_public_receipt_authority(
    policy: &PackageReleasePublicPolicy,
    selection: &PackageReleasePublicSelection,
    snapshot: &PublicReleaseSnapshot,
    deadline: CheckDeadline,
) -> Result<PublicReceiptAuthority, OrchestratorError> {
    let receipt_asset = snapshot
        .assets
        .iter()
        .find(|asset| asset.name == "release.json")
        .ok_or_else(|| internal("package_public_receipt_missing"))?;
    let historical_pin = policy
        .historical
        .iter()
        .find(|pin| pin.tag == selection.tag);
    if let Some(pin) = historical_pin {
        verify_historical_envelope_pin(pin, snapshot, receipt_asset, &policy.plan)?;
    }
    let receipt = velnor_actions_contract::package_release::parse_package_release_receipt(
        &receipt_asset.bytes,
    )
    .map_err(|_| internal("package_public_receipt_invalid"))?;
    verify_receipt_authority(policy, selection, &receipt)?;
    let observed_authority_tip =
        crate::package_release_publish::verify_public_protected_main_lineage(
            &policy.plan.repository,
            policy.repository_id,
            &policy.plan.default_branch,
            &receipt.source.commit_sha,
            &receipt.workflow.authority_sha,
            deadline,
        )?;
    require_policy_authority_tip(&observed_authority_tip, &policy.authority_sha)?;
    let recovery = historical_pin.and_then(|pin| pin.recovery.clone());
    if let Some(recovery) = &recovery {
        let recovery_tip = crate::package_release_publish::verify_public_protected_main_lineage(
            &policy.plan.repository,
            policy.repository_id,
            &policy.plan.default_branch,
            &receipt.workflow.authority_sha,
            &recovery.authority_sha,
            deadline,
        )?;
        require_policy_authority_tip(&recovery_tip, &policy.authority_sha)?;
        require_policy_authority_tip(&recovery_tip, &observed_authority_tip)?;
    }
    if let Some(pin) = historical_pin {
        verify_historical_receipt_pin(pin, &receipt)?;
    }
    Ok(PublicReceiptAuthority {
        receipt,
        historical: historical_pin.is_some(),
        recovery,
    })
}

fn verify_public_release_custody(
    policy: &PackageReleasePublicPolicy,
    selection: &PackageReleasePublicSelection,
    snapshot: &PublicReleaseSnapshot,
    authority: &PublicReceiptAuthority,
    verification_deadline: CheckDeadline,
    custody_deadline: CheckDeadline,
) -> Result<VerifiedAttestedSignedPackage, OrchestratorError> {
    let plan = &policy.plan;
    let files = verified_file_inventory(snapshot, &authority.receipt)?;
    crate::package_release_artifacts::validate_authenticated_release_content(
        plan,
        &authority.receipt,
        &files,
        verification_deadline,
    )?;
    let owner = crate::filesystem_owner::owned_tempdir::private_tempdir_in(
        &std::env::temp_dir(),
        "velnor-public-release-",
    )
    .map_err(|_| internal("package_public_tempdir_create"))?;
    for (name, bytes) in files {
        write_new_file(&owner.path().join(name), &bytes)?;
    }
    let expected = signing_identity(&authority.receipt, plan, &selection.source_sha)?;
    let signed = verify_attested_signed_package_with_owned_tempdir_and_deadlines(
        owner,
        plan,
        &expected,
        verification_deadline,
        custody_deadline,
    )?;
    verify_public_runs(
        policy,
        &authority.receipt,
        &signed,
        authority.recovery.as_ref(),
        verification_deadline,
    )?;
    crate::package_release_publish::verify_publication_snapshot(&signed, snapshot)?;
    verification_deadline
        .remaining()
        .map_err(|_| internal("package_public_verification_deadline"))?;
    custody_deadline
        .remaining()
        .map_err(|_| internal("package_public_custody_deadline"))?;
    signed
        .closure()
        .verify()
        .map_err(|_| internal("package_public_custody_changed"))?;
    Ok(signed)
}

fn verify_public_runs(
    policy: &PackageReleasePublicPolicy,
    receipt: &PackageReleaseReceipt,
    signed: &VerifiedAttestedSignedPackage,
    recovery: Option<&HistoricalPackageReleaseRecovery>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    if let Some(recovery) = recovery {
        crate::package_release_publish::verify_public_historical_release_recovery(
            &policy.plan.repository,
            policy.repository_id,
            &policy.plan.default_branch,
            signed.receipt(),
            &policy.plan,
            recovery,
            deadline,
        )
    } else {
        crate::package_release_publish::verify_public_release_runs(
            &policy.plan.repository,
            policy.repository_id,
            &policy.plan.default_branch,
            receipt,
            deadline,
        )
    }
}

fn verify_receipt_authority(
    policy: &PackageReleasePublicPolicy,
    selection: &PackageReleasePublicSelection,
    receipt: &PackageReleaseReceipt,
) -> Result<(), OrchestratorError> {
    let plan = &policy.plan;
    let workflow = &receipt.workflow;
    let source = &receipt.source;
    if receipt.channel != plan.channel
        || receipt.repository.slug != plan.repository
        || receipt.repository.id != policy.repository_id
        || source.commit_sha != selection.source_sha
        || source.default_branch != plan.default_branch
        || source.source_ref != format!("refs/heads/{}", plan.default_branch)
        || source.event != plan.channel.event()
        || workflow.path != plan.channel.workflow_path()
        || workflow.workflow_ref != source.source_ref
        || workflow.run_id == 0
        || workflow.run_attempt == 0
        || workflow.signer_job_id != plan.channel.signer_job_id()
        || workflow.signer_workflow != plan.channel.workflow_path()
    {
        return Err(internal("package_public_receipt_authority_mismatch"));
    }
    Ok(())
}

pub(super) fn require_policy_authority_tip(
    observed_tip: &str,
    recorded_tip: &str,
) -> Result<(), OrchestratorError> {
    if !is_full_sha(observed_tip) || observed_tip != recorded_tip {
        return Err(internal("package_public_policy_authority_tip_mismatch"));
    }
    Ok(())
}

fn verify_historical_envelope_pin(
    pin: &HistoricalPackageReleaseAdmission,
    snapshot: &PublicReleaseSnapshot,
    receipt_asset: &crate::package_release_publish::PublicReleaseAsset,
    plan: &PackageReleasePlan,
) -> Result<(), OrchestratorError> {
    let plan_digest = sha256(
        &plan
            .canonical_bytes()
            .map_err(|_| internal("package_public_plan_invalid"))?,
    );
    if snapshot.release_id != pin.release_id
        || receipt_asset.id != pin.receipt_asset_id
        || receipt_asset.sha256 != pin.receipt_sha256
        || snapshot.tag != pin.tag
        || snapshot.tag_source_sha != pin.source_sha
        || plan_digest != pin.plan_sha256
    {
        return Err(internal("package_public_historical_release_mismatch"));
    }
    Ok(())
}

fn verify_historical_receipt_pin(
    pin: &HistoricalPackageReleaseAdmission,
    receipt: &PackageReleaseReceipt,
) -> Result<(), OrchestratorError> {
    if receipt.source.commit_sha != pin.source_sha || receipt.tag != pin.tag {
        return Err(internal("package_public_historical_release_mismatch"));
    }
    Ok(())
}
