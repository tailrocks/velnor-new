// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs};

use serde_json::{Value, json};

use super::support::{ExistingTag, Fixture, FixtureProfile, PublicationProcess};

#[derive(Clone, Copy)]
enum RequiredFailure {
    Missing,
    Skipped,
    Failed,
    Duplicate,
    Incomplete,
    IncompletePage,
    WrongRun,
    WrongSha,
    WrongBranch,
}

#[derive(Clone, Copy)]
enum FreshnessChange {
    Main,
    LatestRun,
    AdvanceAttempt,
    FailedRun,
    FailedRequired,
}

#[test]
fn public_publication_requires_one_successful_required_job_with_complete_identity()
-> Result<(), Box<dyn Error>> {
    for profile in [FixtureProfile::Job, FixtureProfile::Runner] {
        for (failure, expected) in [
            (
                RequiredFailure::Missing,
                "image_publication_ci_required_job",
            ),
            (
                RequiredFailure::Skipped,
                "image_publication_ci_required_job",
            ),
            (RequiredFailure::Failed, "image_publication_ci_required_job"),
            (
                RequiredFailure::Duplicate,
                "image_publication_ci_required_job",
            ),
            (
                RequiredFailure::Incomplete,
                "image_publication_ci_job_identity",
            ),
            (
                RequiredFailure::IncompletePage,
                "image_publication_ci_job_inventory",
            ),
            (
                RequiredFailure::WrongRun,
                "image_publication_ci_job_identity",
            ),
            (
                RequiredFailure::WrongSha,
                "image_publication_ci_job_identity",
            ),
            (
                RequiredFailure::WrongBranch,
                "image_publication_ci_run_identity",
            ),
        ] {
            let fixture = Fixture::new_for(profile)?;
            fixture.update_github_state(|state| apply_required_failure(state, failure))?;
            let archive = fs::read(&fixture.archive)?;
            let record = fs::read(&fixture.record)?;
            let foreign = fixture.scratch.join("foreign.keep");
            fs::write(&foreign, b"preserve")?;
            let registry = fixture.preseed_registry_state()?;
            let output = fixture.invoke()?;
            assert_rejected(&fixture, &output, expected)?;
            assert_eq!(fixture.registry_state()?, registry);
            assert_eq!(fs::read(&fixture.archive)?, archive);
            assert_eq!(fs::read(&fixture.record)?, record);
            assert_eq!(fs::read(&foreign)?, b"preserve");
            assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
        }
    }
    Ok(())
}

#[test]
fn public_publication_reads_all_required_ci_job_pages_and_rejects_cross_page_duplicates()
-> Result<(), Box<dyn Error>> {
    for profile in [FixtureProfile::Job, FixtureProfile::Runner] {
        let fixture = Fixture::new_for(profile)?;
        fixture.set_ci_job_pages(json!(valid_second_page(&fixture)))?;
        let output = fixture.invoke()?;
        assert!(
            output.status.success(),
            "later Required page failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: Value = serde_json::from_slice(&fs::read(&fixture.receipt)?)?;
        assert_eq!(receipt["ci_run_id"], profile.ci_run_id());
        assert_eq!(receipt["ci_run_attempt"], 1);
        assert_eq!(
            fixture.registry_digest()?,
            Some(fixture.expected_digest.clone())
        );
        assert_eq!(fixture.stored_inventory()?, fixture.expected_inventory);
        assert!(fixture.last_fixture_error()?.is_none());
    }

    let fixture = Fixture::new()?;
    let mut pages = valid_second_page(&fixture);
    let duplicate = pages[0]["jobs"][0].clone();
    pages[1]["jobs"][0]["id"] = duplicate["id"].clone();
    fixture.set_ci_job_pages(json!(pages))?;
    let archive = fs::read(&fixture.archive)?;
    let record = fs::read(&fixture.record)?;
    let foreign = fixture.scratch.join("foreign.keep");
    fs::write(&foreign, b"preserve")?;
    let registry = fixture.preseed_registry_state()?;
    let output = fixture.invoke()?;
    assert_rejected(&fixture, &output, "image_publication_ci_job_identity")?;
    assert_eq!(fixture.registry_state()?, registry);
    assert_eq!(fs::read(&fixture.archive)?, archive);
    assert_eq!(fs::read(&fixture.record)?, record);
    assert_eq!(fs::read(&foreign)?, b"preserve");
    assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
    Ok(())
}

#[test]
fn public_publication_revalidates_ci_and_main_after_manifest_lookup() -> Result<(), Box<dyn Error>>
{
    for profile in [FixtureProfile::Job, FixtureProfile::Runner] {
        for (change, expected) in [
            (FreshnessChange::Main, "image_publication_main_changed"),
            (FreshnessChange::LatestRun, "image_publication_ci_changed"),
            (
                FreshnessChange::AdvanceAttempt,
                "image_publication_ci_changed",
            ),
            (
                FreshnessChange::FailedRun,
                "image_publication_ci_latest_not_success",
            ),
            (
                FreshnessChange::FailedRequired,
                "image_publication_ci_required_job",
            ),
        ] {
            let fixture = Fixture::new_for(profile)?;
            let archive = fs::read(&fixture.archive)?;
            let record = fs::read(&fixture.record)?;
            let foreign = fixture.scratch.join("foreign.keep");
            fs::write(&foreign, b"preserve")?;
            let registry = fixture.preseed_registry_state()?;
            let mut process: PublicationProcess = fixture.start_at_manifest_lookup()?;
            process.wait_until_blocked()?;
            assert_materialized_layout(&fixture)?;
            fixture.update_github_state(|state| apply_freshness_change(state, change))?;
            let output = process.release_and_wait()?;
            assert_rejected(&fixture, &output, expected)?;
            assert_eq!(fixture.registry_state()?, registry);
            assert_eq!(fs::read(&fixture.archive)?, archive);
            assert_eq!(fs::read(&fixture.record)?, record);
            assert_eq!(fs::read(&foreign)?, b"preserve");
            assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
        }
    }
    Ok(())
}

#[test]
fn public_idempotent_publication_revalidates_before_accepting_existing_tag()
-> Result<(), Box<dyn Error>> {
    for profile in [FixtureProfile::Job, FixtureProfile::Runner] {
        let fixture = Fixture::new_for(profile)?;
        let archive = fs::read(&fixture.archive)?;
        let record = fs::read(&fixture.record)?;
        let foreign = fixture.scratch.join("foreign.keep");
        fs::write(&foreign, b"preserve")?;
        fixture.preseed_registry_state()?;
        fixture.seed_existing_tag(ExistingTag::SameDigest)?;
        let registry = fixture.registry_state()?;
        let mut process: PublicationProcess = fixture.start_at_manifest_lookup()?;
        process.wait_until_blocked()?;
        assert_materialized_layout(&fixture)?;
        fixture.update_github_state(|state| {
            state["ci_jobs"][1]["conclusion"] = json!("failure");
            Ok(())
        })?;
        let output = process.release_and_wait()?;
        assert_rejected(&fixture, &output, "image_publication_ci_required_job")?;
        assert_eq!(fixture.registry_state()?, registry);
        assert_eq!(fs::read(&fixture.archive)?, archive);
        assert_eq!(fs::read(&fixture.record)?, record);
        assert_eq!(fs::read(&foreign)?, b"preserve");
        assert_eq!(fs::read_dir(&fixture.scratch)?.count(), 1);
    }
    Ok(())
}

fn apply_required_failure(
    state: &mut Value,
    failure: RequiredFailure,
) -> Result<(), Box<dyn Error>> {
    if matches!(failure, RequiredFailure::WrongBranch) {
        state["ci_run"]["head_branch"] = json!("release");
        return Ok(());
    }
    let wrong_run = state["ci_run"]["id"]
        .as_u64()
        .ok_or("fixture CI run ID is missing")?
        .checked_add(1)
        .ok_or("fixture CI run ID overflow")?;
    if matches!(failure, RequiredFailure::IncompletePage) {
        let job = state["ci_jobs"]
            .as_array()
            .and_then(|jobs| jobs.first())
            .cloned()
            .ok_or("fixture CI job is missing")?;
        state["ci_job_pages"] = json!([{"total_count":2,"jobs":[job]}]);
        return Ok(());
    }
    let jobs = state["ci_jobs"]
        .as_array_mut()
        .ok_or("fixture CI jobs are missing")?;
    let required_index = jobs
        .iter()
        .position(|job| job["name"] == "Required")
        .ok_or("fixture Required job is missing")?;
    match failure {
        RequiredFailure::Missing => jobs.retain(|job| job["name"] != "Required"),
        RequiredFailure::Skipped => jobs[required_index]["conclusion"] = json!("skipped"),
        RequiredFailure::Failed => jobs[required_index]["conclusion"] = json!("failure"),
        RequiredFailure::Duplicate => {
            let mut duplicate = jobs[required_index].clone();
            duplicate["id"] = json!(999_999_991_u64);
            jobs.push(duplicate);
        }
        RequiredFailure::Incomplete => jobs[required_index]["head_sha"] = Value::Null,
        RequiredFailure::WrongRun => jobs[required_index]["run_id"] = json!(wrong_run),
        RequiredFailure::WrongSha => jobs[required_index]["head_sha"] = json!("f".repeat(40)),
        RequiredFailure::WrongBranch | RequiredFailure::IncompletePage => (),
    }
    Ok(())
}

fn apply_freshness_change(
    state: &mut Value,
    change: FreshnessChange,
) -> Result<(), Box<dyn Error>> {
    match change {
        FreshnessChange::Main => state["main_sha"] = json!("c".repeat(40)),
        FreshnessChange::LatestRun => {
            let current = state
                .get("ci_run")
                .cloned()
                .ok_or("fixture CI run is missing")?;
            let mut latest = current.clone();
            let run_id = latest["id"]
                .as_u64()
                .ok_or("fixture CI run ID is missing")?
                .checked_add(10)
                .ok_or("fixture CI run ID overflow")?;
            let run_number = latest["run_number"]
                .as_u64()
                .ok_or("fixture CI run number is missing")?
                .checked_add(1)
                .ok_or("fixture CI run number overflow")?;
            latest["id"] = json!(run_id);
            latest["run_number"] = json!(run_number);
            state["ci_runs"] = json!([current, latest]);
            let jobs = state["ci_jobs"]
                .as_array_mut()
                .ok_or("fixture CI jobs are missing")?;
            for (index, job) in jobs.iter_mut().enumerate() {
                let id = run_id
                    .checked_add(index as u64 + 1)
                    .ok_or("fixture CI job ID overflow")?;
                job["id"] = json!(id);
                job["run_id"] = json!(run_id);
            }
        }
        FreshnessChange::AdvanceAttempt => state["ci_run"]["run_attempt"] = json!(2),
        FreshnessChange::FailedRun => {
            state["ci_run"]["conclusion"] = json!("failure");
            state["ci_jobs"][0]["conclusion"] = json!("failure");
        }
        FreshnessChange::FailedRequired => state["ci_jobs"][1]["conclusion"] = json!("failure"),
    }
    Ok(())
}

fn valid_second_page(fixture: &Fixture) -> Vec<Value> {
    let first_page = (0..100)
        .map(|index| {
            json!({"id":8_000_000_u64 + index,"run_id":fixture.profile.ci_run_id(),
                "name":format!("Build-{index}"),"head_sha":fixture.source,
                "status":"completed","conclusion":"success"})
        })
        .collect::<Vec<_>>();
    let required = json!({"id":9_000_000_u64,"run_id":fixture.profile.ci_run_id(),
        "name":"Required","head_sha":fixture.source,"status":"completed","conclusion":"success"});
    vec![
        json!({"total_count":101,"jobs":first_page}),
        json!({"total_count":101,"jobs":[required]}),
    ]
}

fn assert_rejected(
    fixture: &Fixture,
    output: &std::process::Output,
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    assert!(!output.status.success(), "accepted invalid CI evidence");
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        diagnostic.contains(expected),
        "expected {expected}; got {diagnostic}"
    );
    assert!(fixture.last_fixture_error()?.is_none());
    assert!(!fixture.receipt.exists());
    Ok(())
}

fn assert_materialized_layout(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    let mut directories = fs::read_dir(&fixture.scratch)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|path| path.is_dir());
    let publication = directories
        .next()
        .ok_or("publication directory missing at manifest lookup")?;
    if directories.next().is_some() || !publication.join("oci/oci-layout").is_file() {
        return Err("manifest lookup did not follow OCI materialization".into());
    }
    Ok(())
}
