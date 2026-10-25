use std::error::Error;

use super::super::action_snapshots::{Actions, action};

type AuthenticatedBinding = (&'static str, &'static str, &'static str);

#[derive(Default)]
struct JobPolicy {
    verifiers: Vec<&'static str>,
    authenticated: Vec<AuthenticatedBinding>,
}

pub(super) fn assert_action_environment_matrix(
    actions: &Actions,
    project_tool_resolver: bool,
) -> Result<(), Box<dyn Error>> {
    for action_name in [
        "generator-release-build-linux",
        "generator-release-build-macos",
        "generator-release-build-macos-intel",
        "generator-release-qualify-linux",
        "generator-release-qualify-macos",
        "generator-release-qualify-macos-intel",
        "generator-release-qualify-homebrew",
    ] {
        let body = action(actions, action_name)?;
        let is_build = action_name.starts_with("generator-release-build-");
        let is_qualifier = action_name.starts_with("generator-release-qualify-");
        let mut verifiers = Vec::new();
        if project_tool_resolver {
            verifiers.push("Resolve project tool versions");
        }
        if is_build || is_qualifier {
            verifiers.push("Verify Rust channel manifest");
        }
        if is_qualifier {
            verifiers.push("Verify jactionlint");
        }
        assert_verifier_inventory(body, &verifiers)?;

        let authenticated = action_bindings(action_name);
        super::super::verifier_snapshots::assert_exact_empty_verifier_credentials_with_bindings(
            body,
            &verifiers,
            &authenticated,
        )?;
    }
    Ok(())
}

fn action_bindings(action: &str) -> Vec<AuthenticatedBinding> {
    match action {
        "generator-release-build-linux"
        | "generator-release-build-macos"
        | "generator-release-build-macos-intel" => vec![(
            "Install pinned Rust and MBX",
            "GITHUB_TOKEN",
            "${{ github.token }}",
        )],
        "generator-release-qualify-homebrew" => vec![(
            "Authenticate historical A and D; install and test preserved Homebrew kegs",
            "GH_TOKEN",
            "${{ github.token }}",
        )],
        _ => Vec::new(),
    }
}

pub(super) fn assert_workflow_environment_matrix(
    workflows: &[&str],
    project_tool_resolver: bool,
) -> Result<(), Box<dyn Error>> {
    let [parent, images, binary, generator] = workflows else {
        return Err("expected coordinator and three family workflows".into());
    };
    let workflow_bodies = [
        ("product-release.yml", *parent),
        ("product-release-images.yml", *images),
        ("product-release-binary.yml", *binary),
        ("product-release-generator.yml", *generator),
    ];
    for (workflow_name, workflow_body) in workflow_bodies {
        assert_workflow_header_is_uncredentialed(workflow_name, workflow_body)?;
        for job_id in super::super::super::job_ids(workflow_body) {
            let job = super::super::super::job_body(workflow_body, job_id)?;
            assert_workflow_job_environment(workflow_name, job_id, job, project_tool_resolver)?;
        }
    }
    Ok(())
}

fn assert_workflow_header_is_uncredentialed(
    workflow_name: &str,
    workflow_body: &str,
) -> Result<(), Box<dyn Error>> {
    let root = workflow_body
        .split_once("jobs:")
        .map(|(header, _)| header)
        .ok_or_else(|| format!("{workflow_name} has no jobs mapping"))?;
    super::super::verifier_snapshots::assert_exact_empty_verifier_credentials_with_bindings(
        root,
        &[],
        &[],
    )
}

fn assert_workflow_job_environment(
    workflow_name: &str,
    job_id: &str,
    job: &str,
    project_tool_resolver: bool,
) -> Result<(), Box<dyn Error>> {
    let mut policy = workflow_job_policy(workflow_name, job_id);
    if project_tool_resolver && resolver_job(workflow_name, job_id) {
        policy.verifiers.insert(0, "Resolve project tool versions");
    }
    assert_verifier_inventory(job, &policy.verifiers)?;
    super::super::verifier_snapshots::assert_exact_empty_verifier_credentials_with_bindings(
        job,
        &policy.verifiers,
        &policy.authenticated,
    )
}

fn workflow_job_policy(workflow: &str, job: &str) -> JobPolicy {
    let mut policy = JobPolicy::default();
    match (workflow, job) {
        ("product-release.yml", "release-eligibility") => {
            policy.verifiers.push("Verify jactionlint");
            policy.authenticated.push((
                "Verify main and latest Required CI",
                "GH_TOKEN",
                "${{ github.token }}",
            ));
        }
        ("product-release.yml", "prepare-images" | "prepare-binary" | "prepare-generator") => {
            policy.authenticated.push((
                "Reconcile immutable family release",
                "GH_TOKEN",
                "${{ github.token }}",
            ));
        }
        ("product-release-generator.yml", "verify-release-source") => {
            policy.verifiers.push("Verify jactionlint");
            policy.authenticated.extend([
                (
                    "Require successful CI at exact main SHA",
                    "GH_TOKEN",
                    "${{ github.token }}",
                ),
                (
                    "Recheck default-branch source",
                    "GH_TOKEN",
                    "${{ github.token }}",
                ),
            ]);
        }
        (
            "product-release-generator.yml",
            "attest-linux" | "attest-macos" | "attest-macos-intel",
        ) => policy.authenticated.push((
            "Fetch and verify candidate attestation bundles",
            "GH_TOKEN",
            "${{ github.token }}",
        )),
        ("product-release-generator.yml", "attest-manifest") => {
            policy.authenticated.push((
                "Fetch and verify manifest attestation bundle",
                "GH_TOKEN",
                "${{ github.token }}",
            ));
        }
        ("product-release-generator.yml", "publish-generator") => {
            policy.authenticated.extend([
                (
                    "Fetch and verify signed release attestations",
                    "GH_TOKEN",
                    "${{ github.token }}",
                ),
                ("Publish GitHub release", "GH_TOKEN", "${{ github.token }}"),
            ]);
        }
        ("product-release-images.yml", "build-images") => {
            policy.verifiers.push("Verify Rust channel manifest");
            policy.authenticated.push((
                "Install pinned resource-probe build and qualification tools",
                "GITHUB_TOKEN",
                "${{ github.token }}",
            ));
        }
        ("product-release-images.yml", "publish-images")
        | ("product-release-binary.yml", "publish-binary") => {
            policy.authenticated.push((
                "Publish GitHub release",
                "GH_TOKEN",
                "${{ github.token }}",
            ));
        }
        ("product-release-binary.yml", "build-binary") => {
            policy.verifiers.push("Verify Rust channel manifest");
        }
        _ => {}
    }
    policy
}

fn resolver_job(workflow: &str, job: &str) -> bool {
    matches!(
        (workflow, job),
        (
            "product-release.yml",
            "release-eligibility" | "prepare-generator"
        ) | (
            "product-release-generator.yml",
            "verify-release-source" | "candidate-manifest"
        ) | ("product-release-images.yml", "build-images")
            | ("product-release-binary.yml", "build-binary")
    )
}

fn assert_verifier_inventory(body: &str, expected: &[&str]) -> Result<(), Box<dyn Error>> {
    for name in [
        "Resolve project tool versions",
        "Verify Rust channel manifest",
        "Verify jactionlint",
    ] {
        let count = body.matches(&format!("- name: {name}\n")).count();
        let expected_count = usize::from(expected.contains(&name));
        if count != expected_count {
            return Err(format!(
                "expected {expected_count} {name:?} verifier steps, found {count}"
            )
            .into());
        }
    }
    Ok(())
}
