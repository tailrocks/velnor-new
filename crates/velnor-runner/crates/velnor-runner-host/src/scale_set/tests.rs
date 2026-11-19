// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{ScaleSetLookupAction, lookup_action};
use crate::config::HostConfig;
use crate::journal::{
    ExistingScaleSetObservation, Journal, ScaleSetCreateRequest, ScaleSetCreateState,
};

const CONFIG: &str = r#"schema = 2
[[connections]]
connection_id = "conn-test-1"
repository_id = 42
repository = "owner/repo"
credential_ref = "keychain:velnor.test/conn-test-1"
[connections.scale_set]
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
[connections.source_policy]
mode = "branch_restricted"
branches = ["main"]
tag_prefixes = []
events = ["push"]
pull_request_rules = []
[host]
max_jobs = 1
[host.resources]
runner_cpu_millicores = 2000
runner_memory_bytes = 4294967296
dind_cpu_millicores = 1000
dind_memory_bytes = 2147483648
operational_memory_reserve_bytes = 1073741824
[docker]
context = "orbstack"
platform = "linux/amd64"
endpoint = "unix:///docker.sock"
"#;

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn a_matching_set_is_adopted_after_a_lost_create_response() {
    for state in [
        ScaleSetCreateState::Requested,
        ScaleSetCreateState::Uncertain,
    ] {
        assert_eq!(
            lookup_action(state, true, false),
            ScaleSetLookupAction::Adopt
        );
        assert_eq!(
            lookup_action(state, false, false),
            ScaleSetLookupAction::Hold
        );
    }
}

async fn adopt_after_crash(uncertain: bool) -> TestResult<(String, String)> {
    let dir = tempfile::tempdir()?;
    let journal = Journal::open(&dir.path().join("launch.db")).await?;
    journal.bind_engine("engine-test").await?;
    let config = HostConfig::parse(CONFIG)?;
    let binding = config.connections().first().ok_or("connection")?;
    let scoped = journal.connection_scope(binding).await?;
    let request = ScaleSetCreateRequest::new(binding, 1)?;
    let record = scoped.prepare_scale_set_create(&request).await?;
    let claim = scoped
        .claim_scale_set_create(&record)
        .await?
        .ok_or("claim")?;
    if uncertain {
        scoped.mark_scale_set_create_uncertain(claim, None).await?;
    }
    let labels = request.selector_labels().to_vec();
    let observed = ExistingScaleSetObservation::verified(
        &request,
        10,
        request.selector_name(),
        &labels,
        true,
    )?;
    scoped.adopt_scale_set(&request, observed).await?;
    let connection = journal.connection().await?;
    let mut rows = connection
        .query(
            "SELECT state, ownership FROM connection_scale_set_intents",
            (),
        )
        .await?;
    let row = rows.next().await?.ok_or("row")?;
    Ok((row.get::<String>(0)?, row.get::<String>(1)?))
}

#[tokio::test]
async fn a_requested_row_adopts_the_matching_set_it_never_recorded() -> TestResult<()> {
    let (state, ownership) = adopt_after_crash(false).await?;
    assert_eq!((state.as_str(), ownership.as_str()), ("adopted", "adopted"));
    Ok(())
}

#[tokio::test]
async fn an_uncertain_row_adopts_the_matching_set_it_never_recorded() -> TestResult<()> {
    let (state, ownership) = adopt_after_crash(true).await?;
    assert_eq!((state.as_str(), ownership.as_str()), ("adopted", "adopted"));
    Ok(())
}

#[test]
fn an_organization_never_adopts_a_set_it_did_not_create() {
    for state in [ScaleSetCreateState::Prepared, ScaleSetCreateState::NoEffect] {
        assert_eq!(
            lookup_action(state, true, false),
            ScaleSetLookupAction::Adopt
        );
        assert_eq!(lookup_action(state, true, true), ScaleSetLookupAction::Hold);
    }
    assert_eq!(
        lookup_action(ScaleSetCreateState::Created, true, true),
        ScaleSetLookupAction::Verify
    );
    assert_eq!(
        lookup_action(ScaleSetCreateState::Prepared, false, true),
        ScaleSetLookupAction::Create
    );
}

#[test]
fn lookup_action_creates_only_from_rows_with_no_remote_effect() {
    for organization in [false, true] {
        for state in [ScaleSetCreateState::Prepared, ScaleSetCreateState::NoEffect] {
            assert_eq!(
                lookup_action(state, false, organization),
                ScaleSetLookupAction::Create
            );
        }
        for state in [
            ScaleSetCreateState::Requested,
            ScaleSetCreateState::Uncertain,
            ScaleSetCreateState::Created,
            ScaleSetCreateState::Adopted,
        ] {
            assert_eq!(
                lookup_action(state, false, organization),
                ScaleSetLookupAction::Hold
            );
        }
    }
}
