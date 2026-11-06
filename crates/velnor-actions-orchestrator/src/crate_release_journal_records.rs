// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(test)]
use super::JournalOperationRefs;
use super::{
    JournalCompletion, JournalIdentity, JournalIo, JournalReceipt, JournalRecord, JournalStage,
    JournalTag, MAX_RECORD_BYTES, OrchestratorError, TAG_MESSAGE, ensure_no_pending_mutation,
    internal, invalid, read_ref, valid_completion, valid_receipt, valid_sha, validate_identity,
    verify_raw_tag,
};

pub fn record_source_bound_start<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
    source: &std::path::Path,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<String, OrchestratorError> {
    if &receipt.identity != identity {
        return Err(internal("crate_release_journal_source_mismatch"));
    }
    crate::verify_journal_source(source, receipt, git)?;
    record_journal(io, identity, JournalStage::Started, receipt, None)
}

#[cfg(test)]
pub(crate) fn record_unprivileged_completion<J, G, R>(
    input: &crate::crate_release_controller::CrateReleaseObservation<'_, J, G, R>,
    tools: crate::package_release_signer::MeasuredReleaseTools,
) -> Result<String, OrchestratorError>
where
    J: JournalIo + JournalOperationRefs,
    G: crate::crate_release_github::GitHubAdmissionRead,
    R: crate::crate_release_registry::CrateRegistryRead,
{
    crate::crate_release_controller::FrozenCompletion::prepare_unprivileged(
        input,
        tools,
        &crate::UnprivilegedSourceExecution,
    )?
    .record_unprivileged(
        &crate::UnprivilegedSourceExecution,
        input.journal,
        input.github,
        input.registry,
    )
}

pub(crate) fn record_frozen_completion<J: JournalIo>(
    journal: &J,
    validated: crate::crate_release_controller::ValidatedCompletion<'_>,
) -> Result<String, OrchestratorError> {
    let (receipt, completion, before_write) = validated.into_parts();
    record_journal_guarded(
        journal,
        &receipt.identity,
        JournalStage::Completed,
        &receipt,
        Some(&completion),
        before_write,
    )
}

pub(super) fn record_journal<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    stage: JournalStage,
    receipt: &JournalReceipt,
    completion: Option<&JournalCompletion>,
) -> Result<String, OrchestratorError> {
    record_journal_guarded(io, identity, stage, receipt, completion, &|| Ok(()))
}

fn record_journal_guarded<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    stage: JournalStage,
    receipt: &JournalReceipt,
    completion: Option<&JournalCompletion>,
    before_write: &dyn Fn() -> Result<(), OrchestratorError>,
) -> Result<String, OrchestratorError> {
    validate_identity(identity)?;
    if !valid_receipt(receipt, identity)
        || match (stage, completion) {
            (JournalStage::Started, None) => false,
            (JournalStage::Completed, Some(value)) => !valid_completion(receipt, value, identity),
            _ => true,
        }
    {
        return Err(invalid());
    }
    let name = journal_name(identity, stage);
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    if read_ref(io, &identity.repository, &name)
        .map_err(|_| invalid())?
        .is_some()
    {
        let existing = read_record(io, identity, stage)?;
        if existing.receipt == *receipt && existing.completion.as_ref() == completion {
            if stage == JournalStage::Completed {
                let started = read_record(io, identity, JournalStage::Started)?;
                if started.receipt != *receipt {
                    return Err(invalid());
                }
                ensure_no_pending_mutation(io, identity, receipt)?;
            }
            io.check_tag_rules(&identity.repository)
                .map_err(|_| invalid())?;
            return Ok(name);
        }
        return Err(invalid());
    }
    if stage == JournalStage::Completed {
        let started = read_record(io, identity, JournalStage::Started)?;
        if started.receipt != *receipt {
            return Err(invalid());
        }
        ensure_no_pending_mutation(io, identity, receipt)?;
    }
    let record = JournalRecord {
        schema: 1,
        identity: identity.clone(),
        stage,
        receipt: receipt.clone(),
        completion: completion.cloned(),
    };
    let bytes = velnor_actions_contract::canonical_json_bytes(&record).map_err(|_| invalid())?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(invalid());
    }
    let tag_sha = create_content_addressed_tag(io, identity, &name, &bytes)?;
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    before_write()?;
    if io
        .create_ref(&identity.repository, &name, &tag_sha)
        .is_err()
    {
        read_ref(io, &identity.repository, &name)
            .map_err(|_| internal("crate_release_journal_ref_write_ambiguous"))?;
        return Err(internal("crate_release_journal_ref_write_ambiguous"));
    }
    if read_ref(io, &identity.repository, &name)
        .map_err(|_| invalid())?
        .as_deref()
        != Some(tag_sha.as_str())
        || read_record(io, identity, stage)? != record
    {
        return Err(invalid());
    }
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    Ok(name)
}

pub(super) fn create_content_addressed_tag<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    name: &str,
    bytes: &[u8],
) -> Result<String, OrchestratorError> {
    let expected_blob = crate::release_policy_admission::git_blob_sha1(bytes)?;
    let blob = io
        .create_blob(&identity.repository, bytes)
        .map_err(|_| invalid())?;
    if blob != expected_blob {
        return Err(invalid());
    }
    let tag = JournalTag {
        name: name.trim_start_matches("refs/tags/").to_owned(),
        object_type: "blob".to_owned(),
        object_sha: blob,
        message: TAG_MESSAGE.to_owned(),
    };
    let tag_sha = io
        .create_tag_object(&identity.repository, &tag)
        .map_err(|_| invalid())?;
    if !valid_sha(&tag_sha)
        || io
            .get_tag_object(&identity.repository, &tag_sha)
            .map_err(|_| invalid())?
            != tag
    {
        return Err(invalid());
    }
    verify_raw_tag(io, &identity.repository, &tag_sha, &tag)?;
    Ok(tag_sha)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEvidence {
    pub receipt: JournalReceipt,
    pub completion: Option<JournalCompletion>,
}

pub fn inspect_journal<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    stage: JournalStage,
) -> Result<Option<JournalEvidence>, OrchestratorError> {
    validate_identity(identity)?;
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    if read_ref(io, &identity.repository, &journal_name(identity, stage))
        .map_err(|_| invalid())?
        .is_none()
    {
        return Ok(None);
    }
    let record = read_record(io, identity, stage)?;
    Ok(Some(JournalEvidence {
        receipt: record.receipt,
        completion: record.completion,
    }))
}

pub(super) fn read_record<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    stage: JournalStage,
) -> Result<JournalRecord, OrchestratorError> {
    let name = journal_name(identity, stage);
    let tag_sha = read_ref(io, &identity.repository, &name)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if !valid_sha(&tag_sha) {
        return Err(invalid());
    }
    let tag = io
        .get_tag_object(&identity.repository, &tag_sha)
        .map_err(|_| invalid())?;
    if tag.name != name.trim_start_matches("refs/tags/")
        || tag.object_type != "blob"
        || tag.message != TAG_MESSAGE
        || !valid_sha(&tag.object_sha)
    {
        return Err(invalid());
    }
    verify_raw_tag(io, &identity.repository, &tag_sha, &tag)?;
    let bytes = io
        .get_blob(&identity.repository, &tag.object_sha)
        .map_err(|_| invalid())?;
    if bytes.len() > MAX_RECORD_BYTES
        || crate::release_policy_admission::git_blob_sha1(&bytes)? != tag.object_sha
    {
        return Err(invalid());
    }
    let value =
        velnor_actions_contract::strict_json::parse_strict_json_bytes(&bytes, MAX_RECORD_BYTES)
            .map_err(|_| invalid())?;
    let record: JournalRecord = serde_json::from_value(value).map_err(|_| invalid())?;
    if record.schema != 1
        || record.identity != *identity
        || record.stage != stage
        || !valid_receipt(&record.receipt, identity)
        || match (record.stage, &record.completion) {
            (JournalStage::Started, None) => false,
            (JournalStage::Completed, Some(value)) => {
                !valid_completion(&record.receipt, value, identity)
            }
            _ => true,
        }
        || velnor_actions_contract::canonical_json_bytes(&record).map_err(|_| invalid())? != bytes
    {
        return Err(invalid());
    }
    Ok(record)
}

pub(super) fn journal_name(identity: &JournalIdentity, stage: JournalStage) -> String {
    let phase = match stage {
        JournalStage::Started => "publication-started",
        JournalStage::Completed => "reconciled",
    };
    format!(
        "refs/tags/velnor-receipt/{}/{}/{phase}",
        identity.run_id, identity.run_attempt
    )
}
