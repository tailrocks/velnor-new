use std::collections::BTreeSet;
use std::error::Error;

use velnor_actions_contract::{GeneratorReleaseSourceBinding, RoutingWorkflow};

use crate::yaml::render_yaml;

use super::super::Schema2WorkflowRequest;

const SOURCE_SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn request(version: &str) -> Result<Schema2WorkflowRequest, crate::RenderError> {
    Ok(Schema2WorkflowRequest {
        version: version.to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::GeneratorRelease]),
        mbx_qualification: None,
    })
}

fn rendered(version: &str) -> Result<String, Box<dyn Error>> {
    let workflow = super::super::generator_release(&request(version)?)?;
    Ok(render_yaml(&workflow))
}

#[test]
fn renderer_uses_typed_target_builds_and_every_native_proof() -> Result<(), Box<dyn Error>> {
    let binding = GeneratorReleaseSourceBinding::for_current_workflow("0.2.7")?;
    let workflow = rendered("0.2.7")?;

    for (job, runner, target, path, artifact) in [
        (
            "build-linux",
            "ubuntu-22.04",
            "x86_64-unknown-linux-gnu",
            "target/x86_64-unknown-linux-gnu/release/velnor-actions",
            "velnor-actions-0.2.7-x86_64-unknown-linux-gnu",
        ),
        (
            "build-macos",
            "macos-15",
            "aarch64-apple-darwin",
            "target/aarch64-apple-darwin/release/velnor-actions",
            "velnor-actions-0.2.7-aarch64-apple-darwin",
        ),
        (
            "build-macos-x86_64",
            "macos-15-intel",
            "x86_64-apple-darwin",
            "target/x86_64-apple-darwin/release/velnor-actions",
            "velnor-actions-0.2.7-x86_64-apple-darwin",
        ),
    ] {
        let job_body = job_section(&workflow, job)?;
        assert!(job_body.contains(&format!("runs-on: {runner}")));
        assert!(job_body.contains(&format!("--target {target}")));
        assert!(job_body.contains(path));
        assert!(job_body.contains(artifact));
        assert!(job_body.contains(
            "--no-config --no-env --no-hooks exec rust@1.98.1 mr-boxington@1.21.1 -- mbx build"
        ));
        assert!(!job_body.contains("cargo build"));
        assert!(job_body.contains("rustc -vV"));
        assert!(job_body.contains("--version"));
        assert!(job_body.contains("--help"));
        assert!(job_body.contains("Verify current workflow source binding"));
        assert!(job_body.contains("VELNOR_RELEASE_SOURCE_SHA: ${{ github.sha }}"));
        assert!(job_body.contains("GIT_NO_REPLACE_OBJECTS=1 git rev-parse"));
        assert!(job_body.contains("if: github.event_name == 'workflow_dispatch'"));
    }

    assert!(workflow.contains("velnor-actions-0.2.7-x86_64-unknown-linux-gnu.sha256"));
    assert!(workflow.contains("velnor-actions-0.2.7-aarch64-apple-darwin.sha256"));
    assert!(workflow.contains("velnor-actions-0.2.7-x86_64-apple-darwin.sha256"));
    let publish = job_section(&workflow, "publish-generator")?;
    assert!(publish.contains("attest-macos-x86_64"));
    assert!(publish.contains("generator-macos-x86_64-assets"));
    assert!(publish.contains("macos-x86_64-assets"));
    assert!(workflow.contains("--version '0.2.7'"));
    assert!(workflow.contains("generator-release.yml@refs/heads/main"));
    let bound = binding.bind(SOURCE_SHA)?;
    assert_eq!(bound.tag(), format!("generator-{SOURCE_SHA}"));
    assert_eq!(
        bound.final_asset_names(),
        [
            "velnor-actions-0.2.7-x86_64-unknown-linux-gnu",
            "velnor-actions-0.2.7-x86_64-unknown-linux-gnu.sha256",
            "velnor-actions-0.2.7-aarch64-apple-darwin",
            "velnor-actions-0.2.7-aarch64-apple-darwin.sha256",
            "velnor-actions-release-manifest.json",
            "velnor-actions-release-manifest.json.sha256",
        ]
    );
    Ok(())
}

#[test]
fn build_attest_and_publish_jobs_reject_untrusted_workflow_contexts() -> Result<(), Box<dyn Error>>
{
    let workflow = rendered("0.2.7")?;
    let guarded_jobs = [
        "build-linux",
        "attest-linux",
        "build-macos",
        "attest-macos",
        "build-macos-x86_64",
        "attest-macos-x86_64",
        "publish-generator",
    ];
    for job in guarded_jobs {
        assert!(
            job_section(&workflow, job)?.contains(
                "github.event_name == 'workflow_dispatch' && github.repository == 'tailrocks/velnor-new' && github.ref == 'refs/heads/main'"
            ),
            "missing trusted-context guard on {job}"
        );
    }
    let publish = job_section(&workflow, "publish-generator")?;
    assert!(publish.contains("github.workflow_sha == github.sha"));
    assert!(publish.contains("name: Publish GitHub release"));
    assert!(publish.contains("name: Upload verified release metadata"));
    Ok(())
}

fn job_section<'a>(workflow: &'a str, id: &str) -> Result<&'a str, Box<dyn Error>> {
    let marker = format!("  {id}:");
    let start = workflow.find(&marker).ok_or("missing workflow job")?;
    let mut offset = 0;
    let mut end = workflow.len();
    for line in workflow[start..].lines().skip(1) {
        offset += line.len() + 1;
        if line.starts_with("  ") && !line.starts_with("    ") && line.ends_with(':') {
            end = start + offset - line.len() - 1;
            break;
        }
    }
    Ok(&workflow[start..end])
}
