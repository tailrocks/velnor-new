use super::{PackageReleaseChannel, PackageReleaseReceipt, invalid};
use crate::{ContractError, config::is_exact_semver, is_valid_branch_name};

pub(super) fn validate(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    validate_repository(receipt)?;
    validate_source(receipt)?;
    validate_workflow(receipt)?;
    validate_version(receipt)
}

fn validate_repository(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    if receipt.repository.id == 0
        || !crate::vcs::is_safe_github_repository(&receipt.repository.slug)
    {
        return Err(invalid("bad_repository_identity"));
    }
    Ok(())
}

fn validate_source(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    let source = &receipt.source;
    if !hex_string(&source.commit_sha, 40)
        || !is_valid_branch_name(&source.default_branch)
        || (receipt.channel == PackageReleaseChannel::Preview && source.default_branch != "main")
        || source.source_ref != format!("refs/heads/{}", source.default_branch)
        || source.ci_run_id == 0
        || source.ci_run_attempt == 0
        || source.event != receipt.channel.event()
    {
        return Err(invalid("bad_source_identity"));
    }
    Ok(())
}

fn validate_workflow(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    let workflow = &receipt.workflow;
    if workflow.path != receipt.channel.workflow_path()
        || workflow.workflow_ref != receipt.source.source_ref
        || !hex_string(&workflow.authority_sha, 40)
        || workflow.authority_sha != receipt.source.commit_sha
        || workflow.run_id == 0
        || workflow.run_attempt == 0
        || workflow.producer_run_id == 0
        || workflow.producer_run_attempt == 0
        || workflow.producer_run_id != workflow.run_id
        || workflow.producer_run_attempt != workflow.run_attempt
        || workflow.signer_job_id != receipt.channel.signer_job_id()
        || workflow.signer_workflow != receipt.channel.workflow_path()
    {
        return Err(invalid("bad_workflow_identity"));
    }
    Ok(())
}

fn validate_version(receipt: &PackageReleaseReceipt) -> Result<(), ContractError> {
    let version = &receipt.version;
    if !is_exact_semver(&version.base) {
        return Err(invalid("bad_release_version_identity"));
    }
    match receipt.channel {
        PackageReleaseChannel::Preview => {
            let (Some(count), Some(sha7)) = (version.commit_count, version.source_sha7.as_deref())
            else {
                return Err(invalid("preview_version_identity_missing"));
            };
            if count == 0
                || sha7.len() != 7
                || !hex_string(sha7, 7)
                || !receipt.source.commit_sha.starts_with(sha7)
                || version.value != format!("{}-preview.{count}+{sha7}", version.base)
                || receipt.tag != format!("v{}", version.value)
            {
                return Err(invalid("version_does_not_match_preview_policy"));
            }
        }
        PackageReleaseChannel::Stable => {
            if version.commit_count.is_some()
                || version.source_sha7.is_some()
                || version.value != version.base
                || receipt.products.len() != 1
                || receipt.tag != format!("{}-v{}", receipt.products[0].package, version.base)
            {
                return Err(invalid("version_does_not_match_stable_policy"));
            }
        }
    }
    Ok(())
}

fn hex_string(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
