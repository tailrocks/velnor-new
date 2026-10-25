use super::action_snapshots::{Actions, action};
use std::error::Error;

#[path = "schema2_generator_release_environment_matrix.rs"]
mod environment_matrix;
#[path = "schema2_generator_release_job_boundary_tests.rs"]
mod job_boundary_tests;

#[cfg(test)]
#[path = "schema2_generator_release_security_credential_tests.rs"]
mod credential_environment_tests;
#[path = "schema2_generator_release_security_credentials.rs"]
mod credentials;
#[path = "schema2_generator_release_security_source.rs"]
mod source;

pub(super) fn assert_exact_empty_verifier_credentials(
    body: &str,
    verifier_steps: &[&str],
) -> Result<(), Box<dyn Error>> {
    credentials::assert_exact_empty_verifier_credentials(body, verifier_steps)
}

pub(super) fn assert_release_workflow_verifier(
    workflow: &str,
    step_name: &str,
    globally_scoped_key: &str,
) -> Result<(), Box<dyn Error>> {
    credentials::assert_release_workflow_verifier(workflow, step_name, globally_scoped_key)
}

#[derive(Clone, Copy)]
struct TargetJobs {
    target: &'static str,
    build: &'static str,
    build_action: &'static str,
    qualify: &'static str,
    qualifier_action: &'static str,
    attest: &'static str,
}

pub(super) fn assert_isolated_candidate_execution(
    generator_workflow: &str,
    workflows: &[&str],
    actions: &Actions,
    project_tool_resolver: bool,
) -> Result<(), Box<dyn Error>> {
    environment_matrix::assert_action_environment_matrix(actions, project_tool_resolver)?;
    environment_matrix::assert_workflow_environment_matrix(workflows, project_tool_resolver)?;
    assert_privileged_jobs_are_source_free(generator_workflow)?;
    for target in [
        TargetJobs {
            target: "linux",
            build: "build-linux",
            build_action: "generator-release-build-linux",
            qualify: "qualify-linux",
            attest: "attest-linux",
            qualifier_action: "generator-release-qualify-linux",
        },
        TargetJobs {
            target: "macos",
            build: "build-macos",
            build_action: "generator-release-build-macos",
            qualify: "qualify-macos",
            attest: "attest-macos",
            qualifier_action: "generator-release-qualify-macos",
        },
        TargetJobs {
            target: "macos-intel",
            build: "build-macos-intel",
            build_action: "generator-release-build-macos-intel",
            qualify: "qualify-macos-intel",
            attest: "attest-macos-intel",
            qualifier_action: "generator-release-qualify-macos-intel",
        },
    ] {
        assert_target_boundary(generator_workflow, actions, target, project_tool_resolver)?;
    }
    job_boundary_tests::candidate_environment_writes_cannot_cross_job_boundary()?;
    Ok(())
}

fn assert_target_boundary(
    workflow: &str,
    actions: &Actions,
    target: TargetJobs,
    project_tool_resolver: bool,
) -> Result<(), Box<dyn Error>> {
    assert_build_artifact_outputs(workflow, target.build)?;
    let build_action = action(actions, target.build_action)?;
    assert_resolver_count(build_action, project_tool_resolver)?;
    let build_verifiers = if project_tool_resolver {
        &[
            "Resolve project tool versions",
            "Verify Rust channel manifest",
        ][..]
    } else {
        &["Verify Rust channel manifest"][..]
    };
    super::verifier_snapshots::assert_exact_empty_verifier_credentials_with_authenticated_binding(
        build_action,
        build_verifiers,
        "Install pinned Rust and MBX",
        "GITHUB_TOKEN",
        "${{ github.token }}",
    )?;
    assert_qualifier_boundary(workflow, actions, target, project_tool_resolver)?;
    super::attester_boundary_snapshots::assert_attester_boundary(
        workflow,
        target.attest,
        target.build,
        target.qualify,
    )
}

fn assert_privileged_jobs_are_source_free(workflow: &str) -> Result<(), Box<dyn Error>> {
    for id in [
        "attest-linux",
        "attest-macos",
        "attest-macos-intel",
        "attest-manifest",
        "publish-generator",
    ] {
        let job = super::super::job_body(workflow, id)?;
        for forbidden in [
            "actions/checkout@",
            "uses: ./.github/actions/",
            "scripts/",
            "cargo ",
            "rustc ",
            "mbx build",
            "git rev-parse HEAD",
            "--version",
            "./linux-assets/",
            "./macos-assets/",
            "./macos-intel-assets/",
            "./target/",
            "Resolve project tool versions",
            "VELNOR_TOOL_",
        ] {
            assert!(!job.contains(forbidden), "{id} contains {forbidden}: {job}");
        }
        assert!(job.contains("candidate-manifest"), "{id}: {job}");
        assert!(
            job.contains(
                "VELNOR_RELEASE_GH_VERSION: ${{ needs.candidate-manifest.outputs.gh_version }}"
            ),
            "{id} must bind authenticated GH authority: {job}"
        );
        assert!(
            job.contains("Validate source-bound GitHub CLI version"),
            "{id}: {job}"
        );
        assert!(
            job.contains(
                r#"mise --no-config --no-env --no-hooks install 'gh@'\"$VELNOR_RELEASE_GH_VERSION\""#
            ),
            "{id}: {job}"
        );
        assert!(job.contains(r#"mise --no-config --no-env --no-hooks exec 'gh@'\"$VELNOR_RELEASE_GH_VERSION\" -- gh"#), "{id}: {job}");
        assert!(
            job.find("Validate source-bound GitHub CLI version")
                < job.find("Install source-bound GitHub CLI"),
            "{id} must validate GH before install"
        );
    }
    Ok(())
}

fn assert_build_artifact_outputs(workflow: &str, build: &str) -> Result<(), Box<dyn Error>> {
    let build_job = super::super::job_body(workflow, build)?;
    assert!(
        build_job.contains("outputs:\n      artifact_id:"),
        "{build_job}"
    );
    assert!(!build_job.contains("artifact_digest:"), "{build_job}");
    assert!(build_job.contains("id: upload\n"), "{build_job}");
    assert!(build_job.contains("artifact-id"), "{build_job}");
    let target = match build {
        "build-linux" => "x86_64-unknown-linux-gnu",
        "build-macos" => "aarch64-apple-darwin",
        "build-macos-intel" => "x86_64-apple-darwin",
        _ => return Err(format!("unknown product build job: {build}").into()),
    };
    let binary = format!("velnor-actions-{}-{target}", env!("CARGO_PKG_VERSION"));
    for asset in [
        binary.clone(),
        format!("{binary}.sha256"),
        format!("{binary}.provenance.json"),
    ] {
        assert!(
            build_job.contains(&asset),
            "{build} omits raw asset {asset}: {build_job}"
        );
    }
    assert!(
        !build_job.contains(".tar"),
        "{build} must upload raw files: {build_job}"
    );
    Ok(())
}

fn assert_qualifier_boundary(
    workflow: &str,
    actions: &Actions,
    target: TargetJobs,
    project_tool_resolver: bool,
) -> Result<(), Box<dyn Error>> {
    let qualifier = super::super::job_body(workflow, target.qualify)?;
    assert_qualifier_job_permissions(qualifier, target);
    assert_qualifier_job_inputs(qualifier, target)?;
    let qualifier_body = action(actions, target.qualifier_action)?;
    assert_resolver_count(qualifier_body, project_tool_resolver)?;
    assert_qualifier_action_boundary(qualifier_body)?;
    let verifier_steps = if project_tool_resolver {
        &[
            "Resolve project tool versions",
            "Verify Rust channel manifest",
            "Verify jactionlint",
        ][..]
    } else {
        &["Verify Rust channel manifest", "Verify jactionlint"][..]
    };
    super::verifier_snapshots::assert_exact_empty_verifier_credentials(
        qualifier_body,
        verifier_steps,
    )?;
    assert_job_action(workflow, target.qualify, target.qualifier_action)?;
    Ok(())
}

fn assert_resolver_count(body: &str, expected: bool) -> Result<(), Box<dyn Error>> {
    let found = body
        .matches("- name: Resolve project tool versions")
        .count();
    let expected = usize::from(expected);
    if found != expected {
        return Err(
            format!("expected {expected} project-tool resolver steps, found {found}").into(),
        );
    }
    Ok(())
}

fn assert_qualifier_job_permissions(qualifier: &str, target: TargetJobs) {
    assert!(qualifier.contains("permissions: {}"), "{qualifier}");
    for permission in [
        "actions: write",
        "contents: write",
        "id-token: write",
        "attestations: write",
    ] {
        assert!(
            !qualifier.contains(permission),
            "{}: {qualifier}",
            target.target
        );
    }
    assert!(
        qualifier.contains(&format!("- {}\n", target.build)),
        "{qualifier}"
    );
    assert!(!qualifier.contains("outputs:"), "{qualifier}");
    assert!(!qualifier.contains("persist-credentials:"), "{qualifier}");
    assert!(!qualifier.contains("actions/checkout@"), "{qualifier}");
}

fn assert_qualifier_job_inputs(qualifier: &str, target: TargetJobs) -> Result<(), Box<dyn Error>> {
    assert!(
        qualifier.contains(&format!(
            "artifact_id: ${{{{ needs.{}.outputs.artifact_id }}}}",
            target.build
        )),
        "{qualifier}"
    );
    assert_eq!(qualifier.matches("- name:").count(), 2, "{qualifier}");
    let first_step = qualifier
        .split("- name:")
        .nth(1)
        .ok_or("qualifier has no first step")?
        .split("- name:")
        .next()
        .ok_or("qualifier first step is missing")?;
    assert!(first_step.contains("shell: bash"), "{first_step}");
    assert!(first_step.contains("run:"), "{first_step}");
    assert!(
        first_step.contains("Fetch exact public qualification source without credentials"),
        "{first_step}"
    );
    let source_run = source::decode_run_script(first_step)?;
    source::assert_credential_free_exact_source(&source_run);
    assert!(!first_step.contains("uses:"), "{first_step}");
    assert!(!source_run.contains("python3"), "{source_run}");
    let source_prepare = qualifier
        .find("Fetch exact public qualification source without credentials")
        .ok_or("missing token-free source preparation")?;
    let composite_call = qualifier
        .find(&format!(
            "uses: ./.github/actions/{}",
            target.qualifier_action
        ))
        .ok_or("missing qualification composite call")?;
    assert!(source_prepare < composite_call, "{qualifier}");
    Ok(())
}

fn assert_qualifier_action_boundary(qualifier_body: &str) -> Result<(), Box<dyn Error>> {
    assert_action_input(qualifier_body);
    assert!(
        qualifier_body.contains("artifact-ids: ${{ inputs.artifact_id }}"),
        "{qualifier_body}"
    );
    assert!(!qualifier_body.contains("${{ needs."), "{qualifier_body}");
    assert!(!qualifier_body.contains("post:"), "{qualifier_body}");
    assert!(!qualifier_body.contains("github_token"), "{qualifier_body}");
    assert_eq!(
        qualifier_body.matches("uses:").count(),
        3,
        "{qualifier_body}"
    );
    assert!(
        qualifier_body.contains("actions/download-artifact@"),
        "{qualifier_body}"
    );
    assert!(
        qualifier_body.contains("artifact-ids: ${{ inputs.manifest_artifact_id }}"),
        "{qualifier_body}"
    );
    assert!(
        qualifier_body.contains("Qualify downloaded candidate"),
        "{qualifier_body}"
    );
    assert_eq!(
        qualifier_body
            .matches("Qualify downloaded candidate")
            .count(),
        1
    );
    assert!(
        !qualifier_body.contains("Attest built artifacts"),
        "{qualifier_body}"
    );
    assert!(
        !qualifier_body.contains("actions/upload-artifact@"),
        "{qualifier_body}"
    );
    let candidate = qualifier_body
        .find("Qualify downloaded candidate")
        .ok_or("missing candidate execution")?;
    assert!(
        !qualifier_body[candidate..].contains("- name:"),
        "{qualifier_body}"
    );
    Ok(())
}

fn assert_action_input(action: &str) {
    assert!(
        action.contains("inputs:\n  artifact_id:\n    description:"),
        "{action}"
    );
    assert!(action.contains("    required: true\n"), "{action}");
}

fn assert_job_action(workflow: &str, id: &str, action_name: &str) -> Result<(), Box<dyn Error>> {
    let job = super::super::job_body(workflow, id)?;
    assert!(
        job.contains(&format!("uses: ./.github/actions/{action_name}")),
        "{job}"
    );
    Ok(())
}
