// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::JobAuthorityLimit as Limit;
use velnor_runner_host::{
    ConnectionId, HostError, KeychainReference, RepositoryConnection, ScaleSetSelector,
    SourcePolicy, SourcePolicyMode, WorkflowEvent, build_job_authority_policy_json,
};

use super::inventory_fixture::{
    TestInventoryContext, TestInventoryWorkflow, inventory_json_from, workflows_of,
};
use super::{SOURCE_SHA, TestResult};

fn connection() -> TestResult<RepositoryConnection> {
    Ok(RepositoryConnection::try_new(
        ConnectionId::parse("conn-test-1")?,
        42,
        "owner/repo".to_owned(),
        KeychainReference::parse("keychain:velnor.test/conn-test-1")?,
        ScaleSetSelector::try_new(
            "ubuntu-26.04-scale-set",
            &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
        )?,
        SourcePolicy::try_new(
            SourcePolicyMode::Repository,
            Vec::new(),
            Vec::new(),
            vec![WorkflowEvent::Push, WorkflowEvent::PullRequest],
            Vec::new(),
        )?,
    )?)
}

fn context(event: &'static str, refs: Vec<&'static str>) -> TestInventoryContext {
    let (branch_filters, pull_request_base_branches) = if event == "push" {
        (refs, Vec::new())
    } else {
        (Vec::new(), refs)
    };
    TestInventoryContext {
        event,
        branch_filters,
        tag_filters: Vec::new(),
        pull_request_base_branches,
    }
}

fn admitted(workflows: &[TestInventoryWorkflow]) -> TestResult<bool> {
    let inventory = inventory_json_from(workflows)?;
    match build_job_authority_policy_json(
        &connection()?,
        &inventory,
        SOURCE_SHA,
        &"d".repeat(64),
        1,
    ) {
        Ok(_) => Ok(true),
        Err(HostError::Config) => Ok(false),
        Err(error) => Err(format!("{error:?}").into()),
    }
}

fn default_workflows(jobs: usize) -> Vec<TestInventoryWorkflow> {
    let permissions = BTreeMap::from([("contents", "read")]);
    workflows_of(&[context("push", vec!["main"])], &permissions, 1, jobs)
}

fn at_and_past(
    limit: Limit,
    shape: impl Fn(usize) -> Vec<TestInventoryWorkflow>,
) -> TestResult<()> {
    let edge = limit.max();
    assert!(admitted(&shape(edge))?, "the host refused {edge} {limit:?}");
    assert!(
        !admitted(&shape(edge + 1))?,
        "the host admitted {} {limit:?}",
        edge + 1
    );
    Ok(())
}

fn leaked(names: impl Iterator<Item = String>) -> Vec<&'static str> {
    names.map(|name| &*name.leak()).collect()
}

#[test]
fn the_host_admits_the_needs_the_generator_can_reach_up_to_the_limit() -> TestResult<()> {
    at_and_past(Limit::Needs, |needs| {
        let mut workflows = default_workflows(needs + 1);
        let jobs = &mut workflows[0].jobs;
        let earlier: Vec<String> = jobs[..needs]
            .iter()
            .map(|job| job.job_key.clone())
            .collect();
        jobs[needs].needs = earlier;
        workflows
    })
}

#[test]
fn the_host_admits_the_refs_the_generator_can_reach_up_to_the_limit() -> TestResult<()> {
    for event in ["push", "pull_request"] {
        at_and_past(Limit::Refs, |count| {
            let refs = leaked((0..count).map(|index| format!("b{index:04}")));
            let mut workflows = default_workflows(1);
            workflows[0].jobs[0].contexts = vec![context(event, refs)];
            workflows
        })?;
    }
    Ok(())
}

#[test]
fn the_host_admits_the_job_key_the_generator_can_reach_up_to_the_limit() -> TestResult<()> {
    at_and_past(Limit::JobKeyBytes, |bytes| {
        let mut workflows = default_workflows(1);
        workflows[0].jobs[0].job_key = "k".repeat(bytes);
        workflows
    })
}

#[test]
fn the_host_admits_the_workflow_path_the_generator_can_reach_up_to_the_limit() -> TestResult<()> {
    at_and_past(Limit::TextBytes, |bytes| {
        let mut workflows = default_workflows(1);
        let path = format!(
            ".github/workflows/{}.yml",
            "f".repeat(bytes - ".github/workflows/.yml".len())
        );
        workflows[0].path.clone_from(&path);
        workflows[0].jobs[0].root_path.clone_from(&path);
        workflows[0].jobs[0].workflow_path = path;
        workflows
    })
}

#[test]
fn the_host_admits_the_ref_text_the_generator_can_reach_up_to_the_limit() -> TestResult<()> {
    at_and_past(Limit::TextBytes, |bytes| {
        let branch = "b".repeat(bytes - "refs/heads/".len());
        let mut workflows = default_workflows(1);
        workflows[0].jobs[0].contexts = vec![context("push", vec![branch.leak()])];
        workflows
    })
}
