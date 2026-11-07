// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{HistoricalAuthority, JournalRegistryFake, Registry};
use super::{CrateReleaseIntent, Error, GitHubAdmissionRequest, Path};
use crate::crate_release_journal::{
    JournalReceipt, fixture_storage::seed_started_record, record_journal_intent,
};
use crate::release::package_release_signer::{AdmittedStart, record_admitted_start};

type OlderPolicy = Option<(CrateReleaseIntent, String)>;

pub(super) fn prepare_chain(
    fixture: &super::super::historical_authority::HistoricalFixture,
    two_link: bool,
) -> Result<(CrateReleaseIntent, String, OlderPolicy), Box<dyn Error>> {
    let mut previous = fixture.previous_intent.clone();
    if !two_link {
        return Ok((previous, fixture.prior_sha.clone(), None));
    }
    let older = (previous.clone(), fixture.prior_sha.clone());
    previous.journal_predecessor = Some(velnor_actions_contract::config::CrateReleasePredecessor {
        run_id: 10,
        run_attempt: 1,
        workflow_sha: fixture.prior_sha.clone(),
    });
    let prior = super::super::commit_history_intent(
        fixture.workflow.path(),
        &previous,
        "signed intermediate",
    )?;
    Ok((previous, prior, Some(older)))
}

pub(super) fn seed_older(
    journal: &JournalRegistryFake,
    prior: &mut JournalReceipt,
    request: &GitHubAdmissionRequest,
    admission: &crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    source: &Path,
) -> Result<Option<String>, Box<dyn Error>> {
    let older: OlderPolicy = serde_json::from_str(&std::env::var("VELNOR_CONTROLLER_OLDER")?)?;
    let Some((intent, workflow)) = older else {
        return Ok(None);
    };
    let (_, mut receipt) = super::super::fixture_journal_plan(source, &intent, request, admission)?;
    receipt.identity.run_id = 10;
    receipt.identity.workflow_sha.clone_from(&workflow);
    receipt.workflow_run_url = "https://github.com/example/demo/actions/runs/10".into();
    receipt
        .generator_version
        .clone_from(&intent.generator_version);
    receipt
        .release_plz_version
        .clone_from(&intent.release_plz_version);
    receipt.rust_version.clone_from(&intent.rust_version);
    super::pin_previous_tools(&mut receipt, &intent)?;
    seed_started_record(journal, &receipt.identity, &receipt)?;
    record_journal_intent(
        journal,
        &receipt.identity,
        &receipt,
        &super::super::fixture_upload_intention(),
    )?;
    prior.predecessor = Some(receipt.identity);
    Ok(Some(workflow))
}

pub(super) fn verify_two_link(
    input: &AdmittedStart<'_, JournalRegistryFake, HistoricalAuthority<'_>, Registry>,
    prior: &JournalReceipt,
) -> Result<(), Box<dyn Error>> {
    verify_forged_digest(input, prior)?;
    super::fs::write(
        input
            .workflow_root
            .join(".github/velnor-release-intent.toml"),
        "untrusted dirty file",
    )?;
    record_admitted_start(input)?;
    super::super::assert_two_link_recovery(input, prior)?;
    Ok(())
}

fn verify_forged_digest(
    input: &AdmittedStart<'_, JournalRegistryFake, HistoricalAuthority<'_>, Registry>,
    prior: &JournalReceipt,
) -> Result<(), Box<dyn Error>> {
    let journal = JournalRegistryFake::default();
    *journal.refs.borrow_mut() = input.journal.refs.borrow().clone();
    *journal.blobs.borrow_mut() = input.journal.blobs.borrow().clone();
    *journal.tags.borrow_mut() = input.journal.tags.borrow().clone();
    let prefix = format!(
        "refs/tags/velnor-receipt/{}/{}/",
        prior.identity.run_id, prior.identity.run_attempt
    );
    journal
        .refs
        .borrow_mut()
        .retain(|name, _| !name.starts_with(&prefix));
    let mut forged = prior.clone();
    forged.intent_sha256 = "d".repeat(64);
    seed_started_record(&journal, &forged.identity, &forged)?;
    record_journal_intent(
        &journal,
        &forged.identity,
        &forged,
        &super::super::fixture_upload_intention(),
    )?;
    let before = journal.write_calls.get();
    let wrong = AdmittedStart {
        journal: &journal,
        ..*input
    };
    let error = record_admitted_start(&wrong).expect_err("forged prior digest must reject");
    assert!(
        error
            .to_string()
            .contains("crate_release_historical_receipt_invalid"),
        "{error}"
    );
    assert_eq!(journal.write_calls.get(), before);
    assert_eq!(input.github.base.writes.get(), 0);
    Ok(())
}
