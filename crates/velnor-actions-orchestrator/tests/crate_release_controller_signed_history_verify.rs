// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) fn verify_frozen_predecessor_drift(
    input: &crate::release::package_release_signer::AdmittedStart<
        '_,
        super::super::JournalRegistryFake,
        super::super::HistoricalAuthority<'_>,
        super::super::Registry,
    >,
    prior: &crate::crate_release_journal::JournalReceipt,
) -> Result<(), Box<dyn Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, FrozenCompletion};
    use crate::crate_release_journal::{JournalStage, inspect_journal};
    let (identity, _) = super::super::fixture_journal_plan(
        input.source,
        input.intent,
        input.request,
        input.admission,
    )?;
    let receipt = inspect_journal(input.journal, &identity, JournalStage::Started)?
        .ok_or("signed current start missing")?
        .receipt;
    *input.registry.archive.borrow_mut() = super::super::fixture_archive(input.source)?;
    input.github.base.tag_exists.set(true);
    input.github.base.release_variant.set(0);
    let observation = CrateReleaseObservation {
        source_execution: input.source_execution,
        git: input.git,
        journal: input.journal,
        identity: &identity,
        journal_receipt: &receipt,
        admission: input.admission,
        intent: input.intent,
        request: input.request,
        github: input.github,
        registry: input.registry,
        source: input.source,
        sequence: 1,
    };
    let freeze = || -> Result<_, Box<dyn Error>> {
        let tools = crate::release::package_release_signer::measure_release_tools(
            &std::path::PathBuf::from(std::env::var("RUNNER_TEMP")?),
            input.intent,
            &receipt,
            input.generator,
        )?;
        Ok(FrozenCompletion::prepare_unprivileged(
            &observation,
            tools,
            &crate::UnprivilegedSourceExecution,
        )?)
    };
    let original_refs = input.journal.refs.borrow().clone();
    freeze()?.record_unprivileged(
        &crate::UnprivilegedSourceExecution,
        input.journal,
        input.github,
        input.registry,
    )?;
    assert!(inspect_journal(input.journal, &identity, JournalStage::Completed)?.is_some());
    *input.journal.refs.borrow_mut() = original_refs;
    for scenario in 0..3 {
        let frozen = freeze()?;
        let refs = input.journal.refs.borrow().clone();
        let writes = input.journal.write_calls.get();
        mutate_predecessor(input.journal, prior, scenario);
        let result = frozen.record_unprivileged(
            &crate::UnprivilegedSourceExecution,
            input.journal,
            input.github,
            input.registry,
        );
        *input.journal.refs.borrow_mut() = refs;
        assert!(result.is_err(), "post-freeze predecessor drift {scenario}");
        assert_eq!(input.journal.write_calls.get(), writes);
        assert!(inspect_journal(input.journal, &identity, JournalStage::Completed)?.is_none());
    }
    Ok(())
}

fn mutate_predecessor(
    journal: &super::super::JournalRegistryFake,
    prior: &crate::crate_release_journal::JournalReceipt,
    scenario: u8,
) {
    let prefix = format!(
        "refs/tags/velnor-receipt/{}/{}",
        prior.identity.run_id, prior.identity.run_attempt
    );
    match scenario {
        0 => {
            journal
                .refs
                .borrow_mut()
                .remove(&format!("{prefix}/publication-started"));
        }
        1 => {
            journal
                .refs
                .borrow_mut()
                .insert(format!("{prefix}/operation-1"), "a".repeat(40));
        }
        _ => {
            journal
                .refs
                .borrow_mut()
                .insert(format!("{prefix}/reconciled"), "a".repeat(40));
        }
    }
}

pub(super) fn verify_real_tool_measurement(
    input: &crate::release::package_release_signer::AdmittedStart<
        '_,
        super::super::JournalRegistryFake,
        super::super::HistoricalAuthority<'_>,
        super::super::Registry,
    >,
    generator_dir: &Path,
) -> Result<(), Box<dyn Error>> {
    use crate::crate_release_journal::{JournalStage, inspect_journal};
    let (identity, _) = super::super::fixture_journal_plan(
        input.source,
        input.intent,
        input.request,
        input.admission,
    )?;
    let receipt = inspect_journal(input.journal, &identity, JournalStage::Started)?
        .ok_or("signed current start missing")?
        .receipt;
    let runner = std::path::PathBuf::from(std::env::var("RUNNER_TEMP")?);
    for (source, relative) in [
        (
            generator_dir.join("velnor-actions-0.1.5-x86_64-unknown-linux-gnu"),
            "velnor/bootstrap/velnor-actions-0.1.5-x86_64-unknown-linux-gnu",
        ),
        (
            "/tools/signed-controller/release-plz".into(),
            "velnor-release-plz/release-plz",
        ),
        (
            "/tools/signed-controller/channel.toml".into(),
            "velnor/rust-manifest/channel.toml",
        ),
    ] {
        let target = runner.join(relative);
        fs::create_dir_all(target.parent().ok_or("tool parent missing")?)?;
        fs::copy(source, target)?;
    }
    crate::release::package_release_signer::measure_release_tools(
        &runner,
        input.intent,
        &receipt,
        input.generator,
    )?;
    Ok(())
}

pub(super) fn verify_prior_observation(
    input: &crate::release::package_release_signer::AdmittedStart<
        '_,
        super::super::JournalRegistryFake,
        super::super::HistoricalAuthority<'_>,
        super::super::Registry,
    >,
    prior: &crate::crate_release_journal::JournalReceipt,
) -> Result<(), Box<dyn Error>> {
    use crate::crate_release_controller::observation::RecoveryState;
    use crate::crate_release_controller::{CrateReleaseObservation, observe_prior_recovery};
    use crate::crate_release_journal::{JournalStage, inspect_journal};
    let (identity, _) = super::super::fixture_journal_plan(
        input.source,
        input.intent,
        input.request,
        input.admission,
    )?;
    let current = inspect_journal(input.journal, &identity, JournalStage::Started)?
        .ok_or("signed current start missing")?
        .receipt;
    let observation = CrateReleaseObservation {
        source_execution: input.source_execution,
        git: input.git,
        journal: input.journal,
        identity: &identity,
        journal_receipt: &current,
        admission: input.admission,
        intent: input.intent,
        request: input.request,
        github: input.github,
        registry: input.registry,
        source: input.source,
        sequence: 2,
    };
    let before = input.journal.write_calls.get();
    let missing = observe_prior_recovery(&observation, &prior.identity)?;
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].state, RecoveryState::Missing);
    assert_eq!(
        serde_json::to_value(&missing[0])?["prior_record"],
        "intention-only"
    );
    assert_eq!(input.journal.write_calls.get(), before);
    *input.registry.archive.borrow_mut() = super::super::fixture_archive(input.source)?;
    input.registry.absent.set(false);
    let present = observe_prior_recovery(&observation, &prior.identity)?;
    assert_eq!(present[0].state, RecoveryState::Present);
    assert_eq!(input.journal.write_calls.get(), before);
    super::super::prior_observation::append_fixture_prior_upload_observation(input.journal, prior)?;
    let after_seed = input.journal.write_calls.get();
    let recorded = observe_prior_recovery(&observation, &prior.identity)?;
    assert_eq!(recorded[0].state, RecoveryState::Present);
    assert_eq!(
        serde_json::to_value(&recorded[0])?["prior_record"],
        "observation-recorded"
    );
    assert_eq!(input.journal.write_calls.get(), after_seed);
    input.github.base.approved.set(false);
    assert!(observe_prior_recovery(&observation, &prior.identity).is_err());
    input.github.base.approved.set(true);
    input.registry.archive.borrow_mut().push(0);
    assert!(observe_prior_recovery(&observation, &prior.identity).is_err());
    assert_eq!(input.journal.write_calls.get(), after_seed);
    Ok(())
}

pub(super) fn verified_generator() -> Result<
    (
        tempfile::TempDir,
        crate::release::package_release_signer::AttestedLinuxGenerator,
    ),
    Box<dyn Error>,
> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/attestation_fixtures");
    let temp = tempfile::tempdir()?;
    let binary = "velnor-actions-0.1.5-x86_64-unknown-linux-gnu";
    for name in [
        "release-manifest.json",
        "release-manifest.json.intoto.jsonl",
        "velnor-actions-0.1.5-x86_64-unknown-linux-gnu.intoto.jsonl",
    ] {
        fs::copy(source.join(name), temp.path().join(name))?;
    }
    let mut decoder = flate2::read::GzDecoder::new(fs::File::open(source.join("binary.gz"))?);
    std::io::copy(
        &mut decoder,
        &mut fs::File::create(temp.path().join(binary))?,
    )?;
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(180))?;
    let (_, _, generator) =
        crate::release::package_release_signer::verified_generator_fixture(temp.path(), deadline)?;
    Ok((temp, generator))
}
