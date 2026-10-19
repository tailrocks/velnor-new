use crate::{RenderError, RenderedFile, extract_workflow_authority};

fn file(source: &str) -> RenderedFile {
    RenderedFile {
        path: ".github/workflows/ci.yml".to_owned(),
        bytes: source.to_owned(),
    }
}

#[test]
fn extracts_exact_trigger_and_job_authority_facts() {
    let workflow = file(
        "name: CI\non:\n  push:\n    branches: [sea/integration]\n  pull_request:\n    branches: [sea/integration]\npermissions:\n  contents: read\nconcurrency:\n  group: ci-${{ github.ref }}\n  cancel-in-progress: ${{ github.event_name == 'pull_request' }}\njobs:\n  test:\n    runs-on: ubuntu-26.04\n    needs: [plan]\n    environment: test\n    steps:\n      - run: echo ${{ secrets.TEST_TOKEN }}\n  plan:\n    runs-on: ubuntu-26.04\n    steps: []\n",
    );
    let result = extract_workflow_authority(&[workflow]).expect("authority facts");
    let workflow = &result[0];
    assert_eq!(workflow.triggers.len(), 2);
    assert_eq!(
        workflow.triggers[0].pull_request_base_branches,
        ["sea/integration"]
    );
    assert_eq!(workflow.triggers[1].branch_filters, ["sea/integration"]);
    let test = workflow
        .jobs
        .iter()
        .find(|job| job.job_key == "test")
        .expect("test job");
    assert!(test.permissions.declared);
    assert_eq!(
        test.permissions.scopes.get("contents").map(String::as_str),
        Some("read")
    );
    assert_eq!(test.needs, ["plan"]);
    assert_eq!(test.secrets, ["TEST_TOKEN"]);
    assert_eq!(test.environments, ["test"]);
    let concurrency = test.concurrency.as_ref().expect("workflow concurrency");
    assert_eq!(concurrency.scope, "workflow");
    assert_eq!(
        concurrency.cancel_in_progress,
        "${{ github.event_name == 'pull_request' }}"
    );
}

#[test]
fn rejects_unbounded_trigger_filters() {
    let workflow = file(
        "on:\n  push:\n    branches: ['release/*']\njobs:\n  test:\n    runs-on: ubuntu-26.04\n    steps: []\n",
    );
    let error = extract_workflow_authority(&[workflow]).expect_err("wildcard must fail");
    assert!(
        matches!(error, RenderError::InvalidWorkflow(problem) if problem.contains("workflow_filter_not_exact"))
    );
}

#[test]
fn rejects_repetition_trigger_filters() {
    let workflow = file(
        "on:\n  push:\n    branches: ['main+']\njobs:\n  test:\n    permissions: {}\n    steps: []\n",
    );
    let error = extract_workflow_authority(&[workflow]).expect_err("plus must fail");
    assert!(error.to_string().contains("workflow_filter_not_exact"));
}

#[test]
fn rejects_unresolved_permission_values() {
    let workflow = file(
        "on: { push: null }\npermissions:\n  contents: ${{ inputs.permission }}\njobs:\n  test:\n    runs-on: ubuntu-26.04\n    steps: []\n",
    );
    let error = extract_workflow_authority(&[workflow]).expect_err("expression must fail");
    assert!(
        matches!(error, RenderError::InvalidWorkflow(problem) if problem.contains("permission_level_invalid"))
    );
}

#[test]
fn rejects_dynamic_environment() {
    let workflow = file(
        "on: { push: null }\npermissions: {}\njobs:\n  test:\n    runs-on: ubuntu-26.04\n    environment: ${{ inputs.environment }}\n    steps: []\n",
    );
    let error = extract_workflow_authority(&[workflow]).expect_err("dynamic environment must fail");
    assert!(
        matches!(&error, RenderError::InvalidWorkflow(problem) if problem.contains("environment_name_unresolved")),
        "{error}"
    );
}

#[test]
fn rejects_dynamic_secret_lookup() {
    let workflow = file(
        "on: { push: null }\npermissions: {}\njobs:\n  test:\n    runs-on: ubuntu-26.04\n    steps:\n      - run: echo ${{ secrets[inputs.name] }}\n",
    );
    let error = extract_workflow_authority(&[workflow]).expect_err("dynamic secret must fail");
    assert!(
        matches!(error, RenderError::InvalidWorkflow(problem) if problem.contains("secret_reference_unresolved"))
    );
}

#[test]
fn reads_workflow_secrets_and_case_insensitive_contexts() {
    let workflow = file(
        "on: {push: null}\nenv:\n  TOKEN: ${{ SECRETS.Inherited_Token }}\njobs:\n  check:\n    permissions: {}\n    steps:\n      - run: echo ${{ SECRETS.Publish_Token }} and ${{ secrets.SECOND_TOKEN }}\n",
    );
    let facts = extract_workflow_authority(&[workflow]).expect("secret names");
    assert_eq!(
        facts[0].jobs[0].secrets,
        ["Inherited_Token", "Publish_Token", "SECOND_TOKEN"]
    );
}

#[test]
fn rejects_case_insensitive_dynamic_secret_lookup() {
    let workflow = file(
        "on: {push: null}\npermissions: {}\njobs:\n  check:\n    steps:\n      - run: echo ${{ SECRETS[inputs.name] }}\n",
    );
    let error = extract_workflow_authority(&[workflow])
        .expect_err("case-insensitive dynamic secret must fail");
    assert!(error.to_string().contains("secret_reference_unresolved"));
}

#[test]
fn rejects_whole_secret_context_references() {
    let workflow = file(
        "on: {push: null}\njobs:\n  check:\n    permissions: {}\n    steps:\n      - run: echo ${{ toJson(secrets) }}\n",
    );
    let error =
        extract_workflow_authority(&[workflow]).expect_err("whole secret context must fail closed");
    assert!(error.to_string().contains("secret_reference_unresolved"));
}

#[test]
fn rejects_external_reusable_workflows() {
    let workflow = file(
        "on: {push: null}\njobs:\n  build:\n    uses: owner/repo/.github/workflows/build.yml@v1\n",
    );
    let error = extract_workflow_authority(&[workflow])
        .expect_err("external reusable workflow must fail closed");
    assert!(
        error
            .to_string()
            .contains("workflow_reusable_call_external")
    );
}

#[test]
fn accepts_sequence_trigger_facts() {
    let workflow =
        file("on: [push, pull_request]\njobs:\n  check:\n    permissions: {}\n    steps: []\n");
    let facts = extract_workflow_authority(&[workflow]).expect("sequence triggers");
    assert_eq!(facts[0].triggers.len(), 2);
}
