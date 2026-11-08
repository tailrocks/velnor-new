// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    JournalIdentity, JournalIo, JournalReceipt, JournalRecord, JournalStage, JournalTag,
    TAG_MESSAGE,
};

pub(crate) fn seed_started_record<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
) -> Result<String, crate::OrchestratorError> {
    let record = JournalRecord {
        schema: super::receipt_schema(receipt)
            .ok_or_else(|| crate::internal::internal("fixture_schema_invalid"))?,
        identity: identity.clone(),
        stage: JournalStage::Started,
        receipt: receipt.clone(),
        completion: None,
    };
    let bytes = velnor_actions_contract::canonical_json_bytes(&record)?;
    let blob = io.create_blob(&identity.repository, &bytes)?;
    let name = format!(
        "refs/tags/velnor-receipt/{}/{}/publication-started",
        identity.run_id, identity.run_attempt
    );
    let tag = JournalTag {
        name: name.trim_start_matches("refs/tags/").to_owned(),
        object_type: "blob".to_owned(),
        object_sha: blob,
        message: TAG_MESSAGE.to_owned(),
    };
    let sha = io.create_tag_object(&identity.repository, &tag)?;
    io.create_ref(&identity.repository, &name, &sha)?;
    Ok(name)
}

pub(crate) fn seed_mutation_record<I: JournalIo>(
    io: &I,
    identity: &JournalIdentity,
    receipt: &JournalReceipt,
    mutation: &super::JournalMutation,
) -> Result<(), crate::OrchestratorError> {
    let bytes = super::fixture_mutation_record_bytes(receipt, mutation.clone())?;
    let name = super::mutation_name(identity, mutation.sequence);
    let sha = super::create_content_addressed_tag(io, identity, &name, &bytes)?;
    io.create_ref(&identity.repository, &name, &sha)?;
    Ok(())
}
