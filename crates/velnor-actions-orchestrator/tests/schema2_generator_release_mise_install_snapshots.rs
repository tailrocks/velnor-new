use super::action_snapshots::{Actions, action};

pub(super) fn assert_build_mise_token_scope(
    body: &str,
    actions: &Actions,
) -> Result<(), Box<dyn std::error::Error>> {
    for (job_id, action_id) in [
        ("build-linux", "generator-release-build-linux"),
        ("build-macos", "generator-release-build-macos"),
        ("build-macos-intel", "generator-release-build-macos-intel"),
    ] {
        let job = super::super::job_body(body, job_id)?;
        assert!(job.contains("actions: write"), "{job_id}: {job}");
        assert!(job.contains("contents: read"), "{job_id}: {job}");
        assert!(!job.contains("contents: write"), "{job_id}: {job}");
        assert_action_token_scope(actions, action_id)?;
    }
    Ok(())
}

fn assert_action_token_scope(
    actions: &Actions,
    action_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let action = action(actions, action_id)?;
    let install = action
        .split("    - name: Install pinned Rust and MBX\n")
        .nth(1)
        .and_then(|step| step.split("\n    - name:").next())
        .ok_or_else(|| format!("{action_id} is missing its Mise install step"))?;
    assert!(
        install.starts_with(
            "      env:\n        GITHUB_TOKEN: ${{ github.token }}\n      shell: bash\n"
        ),
        "{action_id} install must receive only the workflow token: {install}"
    );
    assert!(
        install.contains("GITHUB_TOKEN:?missing workflow token"),
        "{action_id} install must fail before running Mise without a token: {install}"
    );
    assert_eq!(
        action.matches("GITHUB_TOKEN: ${{ github.token }}").count(),
        1,
        "{action_id} must not propagate the token to another step: {action}"
    );
    let build = action
        .split("    - name: Build velnor-actions with MBX\n")
        .nth(1)
        .ok_or_else(|| format!("{action_id} is missing its build step"))?;
    assert!(
        build.contains("env -u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_RUNTIME_TOKEN -u GITHUB_TOKEN"),
        "{action_id} must continue to scrub the token before building: {build}"
    );
    Ok(())
}
