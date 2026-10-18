//! Executable exact-source gate harness shared by stable and preview tests.
use super::*;
use velnor_actions_contract::package_release::PackageReleaseChannel;

pub(in super::super) fn execute(scenario: &Scenario) -> Result<Execution, Box<dyn Error>> {
    execute_channel(scenario, PackageReleaseChannel::Stable)
}

pub(in super::super) fn execute_preview(scenario: &Scenario) -> Result<Execution, Box<dyn Error>> {
    execute_channel(scenario, PackageReleaseChannel::Preview)
}

fn execute_channel(
    scenario: &Scenario,
    channel: PackageReleaseChannel,
) -> Result<Execution, Box<dyn Error>> {
    let mut context = context();
    context.channel = channel;
    let (fixture, git_sha) = create_fixture(scenario)?;
    let path = std::env::var("PATH")?;
    let source_sha = scenario.source_sha.as_deref().unwrap_or(&git_sha);
    let authority_sha = scenario.authority_sha.as_deref().unwrap_or(source_sha);
    let command_output = Command::new("bash")
        .args(["-euo", "pipefail", "-c"])
        .arg(consumer_script(
            &context,
            &["/usr/bin/env".to_owned(), "gh".to_owned()],
            false,
        )?)
        .current_dir(&fixture.repository)
        .env("PATH", format!("{}:{path}", fixture.bin.display()))
        .env("VELNOR_TEST_FIXTURES", &fixture.directory)
        .env("VELNOR_TEST_CALLS", &fixture.calls)
        .env("VELNOR_TEST_REPOSITORY", REPOSITORY)
        .env("GITHUB_REPOSITORY_ID", "100")
        .env(
            "GITHUB_EVENT_PATH",
            fixture.directory.join("completion-event.json"),
        )
        .env("GITHUB_REPOSITORY", &scenario.repository)
        .env("GITHUB_REF", &scenario.ref_name)
        .env("GITHUB_EVENT_NAME", &scenario.event)
        .env("GITHUB_WORKFLOW_REF", &scenario.workflow_ref)
        .env("GITHUB_SHA", source_sha)
        .env("GITHUB_WORKFLOW_SHA", authority_sha)
        .env("GITHUB_OUTPUT", &fixture.output)
        .env("GH_TOKEN", "consumer-test-token")
        .output()?;
    Ok(Execution {
        success: command_output.status.success(),
        stderr: String::from_utf8(command_output.stderr)?,
        output: fs::read_to_string(&fixture.output)?,
        calls: fs::read_to_string(&fixture.calls)?,
    })
}

pub(in super::super) fn assert_rejected(result: &Execution) {
    assert!(
        !result.success,
        "unexpected success; output: {}",
        result.output
    );
    assert!(
        result.output.is_empty(),
        "rejected gate wrote outputs: {}",
        result.output
    );
}

pub(super) fn write_completion_fixtures(
    fixture: &Fixture,
    scenario: &Scenario,
    source_sha: &str,
) -> Result<(), Box<dyn Error>> {
    let run = ci_run(
        42,
        12,
        3,
        SOURCE_TOKEN,
        DEFAULT_BRANCH,
        "push",
        ("completed", "success"),
    );
    let event = format!(
        r#"{{"action":"completed","repository":{{"id":100,"full_name":"{REPOSITORY}"}},"workflow":{{"id":200,"name":"CI","path":".github/workflows/ci.yml"}},"workflow_run":{run}}}"#
    );
    for (file, value) in [
        (
            "completion-event.json",
            scenario.completion_event.as_deref().unwrap_or(&event),
        ),
        (
            "completion-run.json",
            scenario.completion_run.as_deref().unwrap_or(&run),
        ),
        (
            "workflow.json",
            scenario.ci_workflow.as_deref().unwrap_or(
                r#"{"id":200,"name":"CI","path":".github/workflows/ci.yml","state":"active"}"#,
            ),
        ),
    ] {
        fs::write(fixture.directory.join(file), materialize(value, source_sha))?;
    }
    Ok(())
}
