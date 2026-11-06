// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use serde_json::{Value, json};

use super::support::{Fixture, FixtureProfile};

#[test]
fn public_publication_selects_the_attempt_artifact_from_later_pages() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new_for(FixtureProfile::Runner)?;
    let state = fixture.read_github_state()?;
    let artifact = state["artifacts"][0].clone();
    let first = (0..100)
        .map(|offset| {
            let mut filler = artifact.clone();
            filler["id"] = json!(900_000 + offset);
            filler["name"] = json!(format!("unrelated-{offset}"));
            filler
        })
        .collect::<Vec<_>>();
    fixture.update_github_state(|state| {
        state["artifact_pages"] = json!([
            {"total_count":101,"artifacts":first},
            {"total_count":101,"artifacts":[artifact.clone()]}
        ]);
        Ok(())
    })?;
    let output = fixture.invoke()?;
    assert!(
        output.status.success(),
        "later artifact page failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = super::support::read_receipt(&fixture.receipt)?;
    assert_eq!(receipt["artifact_id"], fixture.artifact_id);
    assert_eq!(
        fixture.registry_digest()?,
        Some(fixture.expected_digest.clone())
    );
    assert!(fixture.last_fixture_error()?.is_none());
    Ok(())
}

#[test]
fn public_publication_rejects_incomplete_and_ambiguous_artifact_pages_before_writing()
-> Result<(), Box<dyn Error>> {
    let scenario = "publication_collections::public_publication_rejects_incomplete_and_ambiguous_artifact_pages_before_writing";
    if !super::begin_isolated(scenario)? {
        return Ok(());
    }
    let cases = [
        (
            json!([{"total_count":2001,"artifacts":[]}]),
            "image_publication_artifact_inventory",
        ),
        (
            json!([{"artifacts":[]}]),
            "image_publication_artifact_inventory",
        ),
        (
            json!([
                {"total_count":101,"artifacts":(0..100).map(artifact).collect::<Vec<_>>()},
                {"total_count":101,"artifacts":[]}
            ]),
            "image_publication_artifact_inventory",
        ),
        (
            json!([
                {"total_count":101,"artifacts":(0..100).map(artifact).collect::<Vec<_>>()},
                {"total_count":102,"artifacts":[artifact(101)]}
            ]),
            "image_publication_artifact_inventory",
        ),
    ];
    for (pages, expected) in cases {
        let fixture = Fixture::new_for(FixtureProfile::Runner)?;
        fixture.update_github_state(|state| {
            state["artifact_pages"] = pages;
            Ok(())
        })?;
        assert_rejected_before_write(&fixture, expected)?;
    }
    let fixture = Fixture::new_for(FixtureProfile::Runner)?;
    let state = fixture.read_github_state()?;
    let current = state["artifacts"][0].clone();
    let first = (0..100)
        .map(|offset| artifact(800_000 + offset))
        .collect::<Vec<_>>();
    fixture.update_github_state(|state| {
        state["artifact_pages"] = json!([
            {"total_count":102,"artifacts":first},
            {"total_count":102,"artifacts":[current.clone(), {
                "id":fixture.artifact_id + 1000,
                "name":current["name"],
                "expired":false,
                "size_in_bytes":4096,
                "workflow_run":current["workflow_run"]
            }]}
        ]);
        Ok(())
    })?;
    assert_rejected_before_write(&fixture, "image_publication_artifact_count")?;
    let fixture = Fixture::new_for(FixtureProfile::Runner)?;
    let first = (0..100)
        .map(|offset| artifact(800_000 + offset))
        .collect::<Vec<_>>();
    let duplicate = artifact(800_000);
    fixture.update_github_state(|state| {
        state["artifact_pages"] = json!([
            {"total_count":101,"artifacts":first},
            {"total_count":101,"artifacts":[duplicate]}
        ]);
        Ok(())
    })?;
    assert_rejected_before_write(&fixture, "image_publication_artifact_identity")?;
    super::isolated_worker::record_success("image_publication", scenario)?;
    Ok(())
}

#[test]
fn public_publication_reads_later_attestor_job_pages_and_rejects_duplicate_ids()
-> Result<(), Box<dyn Error>> {
    for (profile, key, expected_name, error) in [
        (
            FixtureProfile::Runner,
            "qualifier_job_pages",
            "qualify-images",
            "image_qualification_proof_job",
        ),
        (
            FixtureProfile::Job,
            "attestor_job_pages",
            "Attest Linux job image candidate",
            "image_job_attestation_job",
        ),
    ] {
        reject_duplicate_attestor_names(profile, key, error)?;
        publish_with_later_attestor_job(profile, key, expected_name)?;
        reject_duplicate_attestor_ids(profile, key)?;
    }
    Ok(())
}

fn reject_duplicate_attestor_names(
    profile: FixtureProfile,
    key: &str,
    error: &str,
) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new_for(profile)?;
    let state = fixture.read_github_state()?;
    let collection = if profile == FixtureProfile::Runner {
        &state["qualifier_jobs"]
    } else {
        &state["jobs"]
    };
    let target = if profile == FixtureProfile::Runner {
        collection[0].clone()
    } else {
        collection[1].clone()
    };
    let first = std::iter::once(target.clone())
        .chain((0..99).map(|offset| {
            json!({
                "id":700_000 + offset,
                "name":format!("unrelated-{offset}"),
                "status":"completed",
                "conclusion":"success"
            })
        }))
        .collect::<Vec<_>>();
    let mut duplicate_target = target.clone();
    let target_id = target["id"].as_u64().ok_or("attestor job ID missing")?;
    duplicate_target["id"] = json!(target_id + 1000);
    let duplicate_name = Fixture::new_for(profile)?;
    duplicate_name.update_github_state(|state| {
        state[key] = json!([
            {"total_count":101,"jobs":first},
            {"total_count":101,"jobs":[duplicate_target]}
        ]);
        Ok(())
    })?;
    assert_rejected_before_write(&duplicate_name, error)?;
    Ok(())
}

fn publish_with_later_attestor_job(
    profile: FixtureProfile,
    key: &str,
    expected_name: &str,
) -> Result<(), Box<dyn Error>> {
    let duplicate_id = Fixture::new_for(profile)?;
    let state = duplicate_id.read_github_state()?;
    let collection = if profile == FixtureProfile::Runner {
        &state["qualifier_jobs"]
    } else {
        &state["jobs"]
    };
    let target = if profile == FixtureProfile::Runner {
        collection[0].clone()
    } else {
        collection[1].clone()
    };
    let first = (0..100)
        .map(|offset| {
            json!({
                "id":700_000 + offset,
                "name":format!("unrelated-{offset}"),
                "status":"completed",
                "conclusion":"success"
            })
        })
        .collect::<Vec<_>>();
    duplicate_id.update_github_state(|state| {
        state[key] = json!([
            {"total_count":101,"jobs":first},
            {"total_count":101,"jobs":[target.clone()]}
        ]);
        Ok(())
    })?;
    let output = duplicate_id.invoke()?;
    assert!(
        output.status.success(),
        "{expected_name} on page 2 failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        duplicate_id.registry_digest()?,
        Some(duplicate_id.expected_digest.clone())
    );
    assert!(duplicate_id.last_fixture_error()?.is_none());
    Ok(())
}

fn reject_duplicate_attestor_ids(profile: FixtureProfile, key: &str) -> Result<(), Box<dyn Error>> {
    let duplicate_id_case = Fixture::new_for(profile)?;
    let first = (0..100)
        .map(|offset| {
            json!({
                "id":700_000 + offset,
                "name":format!("unrelated-{offset}"),
                "status":"completed",
                "conclusion":"success"
            })
        })
        .collect::<Vec<_>>();
    let state = duplicate_id_case.read_github_state()?;
    let collection = if profile == FixtureProfile::Runner {
        &state["qualifier_jobs"]
    } else {
        &state["jobs"]
    };
    let target = if profile == FixtureProfile::Runner {
        collection[0].clone()
    } else {
        collection[1].clone()
    };
    let mut duplicate_target = target;
    duplicate_target["id"] = first[0]["id"].clone();
    duplicate_id_case.update_github_state(|state| {
        state[key] = json!([
            {"total_count":101,"jobs":first},
            {"total_count":101,"jobs":[duplicate_target]}
        ]);
        Ok(())
    })?;
    assert_rejected_before_write(
        &duplicate_id_case,
        if profile == FixtureProfile::Runner {
            "image_qualification_proof_job"
        } else {
            "image_job_attestation_job"
        },
    )?;
    Ok(())
}

#[test]
fn public_publication_selects_latest_matching_ci_run_from_later_page() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new_for(FixtureProfile::Job)?;
    let state = fixture.read_github_state()?;
    let base = state["ci_run"].clone();
    let first = (1..=100)
        .map(|run_number| {
            let mut run = base.clone();
            run["id"] = json!(1_200_000 + run_number);
            run["run_number"] = json!(run_number);
            run
        })
        .collect::<Vec<_>>();
    let mut latest = base;
    latest["id"] = json!(fixture.producer_run_id);
    latest["run_number"] = json!(101);
    let all = first
        .iter()
        .cloned()
        .chain(std::iter::once(latest.clone()))
        .collect::<Vec<_>>();
    fixture.update_github_state(|state| {
        state["ci_runs"] = json!(all);
        state["ci_run_pages"] = json!([
            {"total_count":101,"workflow_runs":first},
            {"total_count":101,"workflow_runs":[latest.clone()]}
        ]);
        for job in state["ci_jobs"].as_array_mut().ok_or("missing CI jobs")? {
            job["run_id"] = latest["id"].clone();
        }
        Ok(())
    })?;
    let output = fixture.invoke()?;
    assert!(
        output.status.success(),
        "later CI run page failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = super::support::read_receipt(&fixture.receipt)?;
    assert_eq!(receipt["ci_run_id"], latest["id"]);
    assert_eq!(
        fixture.registry_digest()?,
        Some(fixture.expected_digest.clone())
    );
    assert!(fixture.last_fixture_error()?.is_none());
    Ok(())
}

#[test]
fn public_publication_rejects_saturated_filtered_ci_run_inventory_before_writing()
-> Result<(), Box<dyn Error>> {
    let scenario = "publication_collections::public_publication_rejects_saturated_filtered_ci_run_inventory_before_writing";
    if !super::begin_isolated(scenario)? {
        return Ok(());
    }
    let fixture = Fixture::new_for(FixtureProfile::Job)?;
    fixture.update_github_state(|state| {
        let prototype = state["ci_run"].clone();
        let pages = (0..10)
            .map(|page| {
                let runs = (0..100)
                    .map(|index| {
                        let mut run = prototype.clone();
                        let number = page * 100 + index + 1;
                        run["id"] = json!(number);
                        run["run_number"] = json!(number);
                        run
                    })
                    .collect::<Vec<_>>();
                json!({"total_count":1000,"workflow_runs":runs})
            })
            .collect::<Vec<_>>();
        state["ci_run_pages"] = json!(pages);
        Ok(())
    })?;
    assert_rejected_before_write(&fixture, "image_publication_ci_run_inventory")?;
    super::isolated_worker::record_success("image_publication", scenario)?;
    Ok(())
}

fn artifact(id: u64) -> Value {
    json!({
        "id":id + 900_000,
        "name":format!("unrelated-{id}"),
        "expired":false,
        "size_in_bytes":4096,
        "workflow_run":{"id":FixtureProfile::Runner.run_id(),"head_sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","head_branch":"main"}
    })
}

fn assert_rejected_before_write(fixture: &Fixture, expected: &str) -> Result<(), Box<dyn Error>> {
    let registry = fixture.preseed_registry_state()?;
    let output = fixture.invoke()?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
    assert!(!fixture.receipt.exists());
    assert_eq!(fixture.registry_state()?, registry);
    assert!(fixture.last_fixture_error()?.is_none());
    Ok(())
}
