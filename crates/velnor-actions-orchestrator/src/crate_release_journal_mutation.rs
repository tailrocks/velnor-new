// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    JournalFileKind, JournalIdentity, JournalIo, JournalMutation, JournalMutationKind,
    JournalMutationPhase, JournalMutationResult, JournalOwnership, JournalPackage, JournalReceipt,
    JournalStage, MAX_RECORD_BYTES, MutationRecord, OrchestratorError, TAG_MESSAGE,
    create_content_addressed_tag, internal, invalid, journal_name, read_record, read_ref, schema,
    valid_receipt, valid_sha, validate_identity, verify_raw_tag,
};

pub fn inspect_journal_mutation<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    sequence: u32,
) -> Result<Option<JournalMutation>, OrchestratorError> {
    validate_identity(identity)?;
    if sequence == 0 || sequence > 1000 {
        return Err(invalid());
    }
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    let started = read_record(io, identity, JournalStage::Started)?;
    let mut previous = None;
    for current in 1..=sequence {
        let Some(record) = read_mutation(io, identity, current)? else {
            return if current == sequence {
                Ok(None)
            } else {
                Err(invalid())
            };
        };
        if record.receipt != started.receipt
            || !valid_mutation(&record.mutation, &started.receipt, previous.as_ref())
        {
            return Err(invalid());
        }
        previous = Some(record.mutation);
    }
    Ok(previous)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalIntendedMutation {
    pub sequence: u32,
    pub package: String,
    pub version: String,
    pub kind: JournalMutationKind,
    pub semver_evidence: Option<crate::crate_release_semver::SemverEvidence>,
}

pub(crate) fn record_journal_intent<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
    intended: &JournalIntendedMutation,
) -> Result<String, OrchestratorError> {
    let mutation = JournalMutation {
        sequence: intended.sequence,
        package: intended.package.clone(),
        version: intended.version.clone(),
        kind: intended.kind,
        semver_evidence: intended.semver_evidence.clone(),
        phase: JournalMutationPhase::Intended,
    };
    record_journal_mutation(io, identity, receipt, &mutation)
}

pub(crate) fn record_verified_observation<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
    observed: crate::crate_release_controller::VerifiedMutation,
) -> Result<String, OrchestratorError> {
    record_journal_mutation(io, identity, receipt, &observed.into_inner())
}

pub(super) fn record_journal_mutation<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
    mutation: &JournalMutation,
) -> Result<String, OrchestratorError> {
    validate_identity(identity)?;
    if mutation.sequence == 0 || mutation.sequence > 1000 {
        return Err(invalid());
    }
    let started = read_record(io, identity, JournalStage::Started)?;
    if started.receipt != *receipt {
        return Err(invalid());
    }
    let previous = if mutation.sequence == 1 {
        None
    } else {
        Some(inspect_journal_mutation(io, identity, mutation.sequence - 1)?.ok_or_else(invalid)?)
    };
    if !valid_mutation(mutation, receipt, previous.as_ref()) {
        return Err(invalid());
    }
    let name = mutation_name(identity, mutation.sequence);
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    if read_ref(
        io,
        &identity.repository,
        &journal_name(identity, JournalStage::Completed),
    )
    .map_err(|_| invalid())?
    .is_some()
    {
        return Err(invalid());
    }
    if read_ref(io, &identity.repository, &name)
        .map_err(|_| invalid())?
        .is_some()
    {
        return if inspect_journal_mutation(io, identity, mutation.sequence)?
            == Some(mutation.clone())
        {
            Ok(name)
        } else {
            Err(invalid())
        };
    }
    let record = MutationRecord {
        schema: 1,
        identity: identity.clone(),
        receipt: receipt.clone(),
        mutation: mutation.clone(),
    };
    let bytes = velnor_actions_contract::canonical_json_bytes(&record).map_err(|_| invalid())?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(invalid());
    }
    let tag = create_content_addressed_tag(io, identity, &name, &bytes)?;
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    if io.create_ref(&identity.repository, &name, &tag).is_err() {
        return Err(internal("crate_release_journal_ref_write_ambiguous"));
    }
    if read_ref(io, &identity.repository, &name)
        .map_err(|_| invalid())?
        .as_deref()
        != Some(tag.as_str())
        || inspect_journal_mutation(io, identity, mutation.sequence)? != Some(mutation.clone())
    {
        return Err(invalid());
    }
    io.check_tag_rules(&identity.repository)
        .map_err(|_| invalid())?;
    Ok(name)
}

pub(super) fn ensure_no_pending_mutation<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
) -> Result<(), OrchestratorError> {
    let mut previous: Option<JournalMutation> = None;
    for sequence in 1..=1000 {
        let Some(record) = read_mutation(io, identity, sequence)? else {
            return match previous {
                None
                | Some(JournalMutation {
                    phase: JournalMutationPhase::Observed { .. },
                    ..
                }) => Ok(()),
                _ => Err(invalid()),
            };
        };
        if &record.receipt != receipt
            || !valid_mutation(&record.mutation, receipt, previous.as_ref())
        {
            return Err(invalid());
        }
        previous = Some(record.mutation);
    }
    Err(invalid())
}

pub(crate) fn valid_journal_phase(phase: &str) -> bool {
    if matches!(phase, "publication-started" | "reconciled") {
        return true;
    }
    phase.strip_prefix("operation-").is_some_and(|value| {
        value
            .parse::<u32>()
            .is_ok_and(|number| (1..=1000).contains(&number) && number.to_string() == value)
    })
}

pub(crate) fn mutation_name(identity: &JournalIdentity, sequence: u32) -> String {
    format!(
        "refs/tags/velnor-receipt/{}/{}/operation-{sequence}",
        identity.run_id, identity.run_attempt
    )
}

pub(super) fn valid_mutation(
    item: &JournalMutation,
    receipt: &JournalReceipt,
    previous: Option<&JournalMutation>,
) -> bool {
    if item.kind == JournalMutationKind::Upload
        && matches!(
            receipt.semver_evidence.get(&item.package),
            Some(crate::crate_release_semver::SemverEvidence::PriorNoBaselinePending { .. })
        )
    {
        return false;
    }
    match (&item.phase, item.kind, &item.semver_evidence) {
        (JournalMutationPhase::Intended, JournalMutationKind::Upload, Some(evidence))
            if evidence.valid_for(&item.version) => {}
        (JournalMutationPhase::Intended, JournalMutationKind::Upload, _) | (_, _, Some(_)) => {
            return false;
        }
        _ => {}
    }
    let Some(package) = receipt
        .packages
        .iter()
        .find(|package| package.name == item.package && package.version == item.version)
    else {
        return false;
    };
    match (&item.phase, previous) {
        (JournalMutationPhase::Intended, None) => item.sequence == 1,
        (JournalMutationPhase::Intended, Some(prior)) => {
            matches!(prior.phase, JournalMutationPhase::Observed { .. })
        }
        (JournalMutationPhase::Observed { result }, Some(prior)) => {
            valid_mutation_result(item.kind, result, package, &receipt.identity)
                && matches!(prior.phase, JournalMutationPhase::Intended)
                && prior.package == item.package
                && prior.version == item.version
                && prior.kind == item.kind
        }
        _ => false,
    }
}

pub(super) fn valid_mutation_result(
    kind: JournalMutationKind,
    result: &JournalMutationResult,
    package: &JournalPackage,
    identity: &JournalIdentity,
) -> bool {
    match (kind, result) {
        (
            JournalMutationKind::Upload,
            JournalMutationResult::Upload {
                registry_checksum,
                downloaded_checksum,
                owners,
                features,
                files,
            },
        ) => {
            schema::valid_sha256(registry_checksum)
                && registry_checksum == downloaded_checksum
                && match &package.owners {
                    JournalOwnership::Existing { owners: expected } => owners == expected,
                    JournalOwnership::NewCrate => schema::valid_owners(owners),
                }
                && schema::valid_files(files)
                && files.windows(2).all(|pair| pair[0].path < pair[1].path)
                && files
                    .iter()
                    .all(|file| matches!(file.kind, JournalFileKind::Regular { .. }))
                && features.len() <= 10_000
                && features.iter().all(|(name, values)| {
                    schema::valid_feature(name)
                        && values.len() <= 10_000
                        && values.iter().all(|value| schema::valid_feature(value))
                })
        }
        (JournalMutationKind::CreateTag, JournalMutationResult::Tag { name, target_sha }) => {
            name == &package.tag && target_sha == &identity.source_sha
        }
        (JournalMutationKind::CreateRelease, JournalMutationResult::Release { id, tag, url }) => {
            *id > 0
                && tag == &package.tag
                && [package.tag.clone(), schema::percent_encode(&package.tag)]
                    .iter()
                    .any(|tag| {
                        url == &format!(
                            "https://github.com/{}/releases/tag/{tag}",
                            identity.repository
                        )
                    })
        }
        _ => false,
    }
}

pub(super) fn read_mutation<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    sequence: u32,
) -> Result<Option<MutationRecord>, OrchestratorError> {
    let name = mutation_name(identity, sequence);
    let Some(tag_sha) = read_ref(io, &identity.repository, &name).map_err(|_| invalid())? else {
        return Ok(None);
    };
    read_mutation_at(io, identity, sequence, &tag_sha).map(Some)
}

pub(super) fn read_mutation_at<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    sequence: u32,
    tag_sha: &str,
) -> Result<MutationRecord, OrchestratorError> {
    let name = mutation_name(identity, sequence);
    if !valid_sha(tag_sha) {
        return Err(invalid());
    }
    let tag = io
        .get_tag_object(&identity.repository, tag_sha)
        .map_err(|_| invalid())?;
    if tag.name != name.trim_start_matches("refs/tags/")
        || tag.object_type != "blob"
        || tag.message != TAG_MESSAGE
        || !valid_sha(&tag.object_sha)
    {
        return Err(invalid());
    }
    verify_raw_tag(io, &identity.repository, tag_sha, &tag)?;
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
    let record: MutationRecord = serde_json::from_value(value).map_err(|_| invalid())?;
    if record.schema != 1
        || record.identity != *identity
        || record.mutation.sequence != sequence
        || !valid_receipt(&record.receipt, identity)
        || velnor_actions_contract::canonical_json_bytes(&record).map_err(|_| invalid())? != bytes
    {
        return Err(invalid());
    }
    Ok(record)
}
