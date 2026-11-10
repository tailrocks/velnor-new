// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, error::Error, io};
use velnor_actions_contract::RoutingWorkflow;
use velnor_actions_workflow_renderer::{Schema2WorkflowRequest, render_schema2_workflows};

fn request(generator: bool) -> Result<Schema2WorkflowRequest, Box<dyn Error>> {
    let mut workflows = BTreeSet::from([RoutingWorkflow::Qualification]);
    if generator {
        workflows.insert(RoutingWorkflow::GeneratorRelease);
    }
    Ok(Schema2WorkflowRequest {
        runner_image_family: None,
        version: "0.1.79".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows,
        mbx_qualification: Some(super::gc1_t50::mbx_pins()),
        mise_pin_qualification: Some(super::gc1_t50::mise_pin_pins()),
        product_release: generator.then(super::gc1_t42::pins),
    })
}

fn section<'a>(text: &'a str, id: &str) -> Result<&'a str, Box<dyn Error>> {
    let start = text
        .find(&format!("\n  {id}:\n"))
        .ok_or_else(|| io::Error::other("missing job"))?
        + 1;
    let rest = &text[start..];
    let mut offset = 0;
    for line in rest.split_inclusive('\n').skip(1) {
        offset += line.len();
        if line.starts_with("  ") && !line.starts_with("    ") {
            let first = rest
                .find('\n')
                .ok_or_else(|| io::Error::other("missing job newline"))?
                + 1;
            return Ok(&rest[..first + offset - line.len()]);
        }
    }
    Ok(rest)
}

fn workflow(generator: bool) -> Result<String, Box<dyn Error>> {
    let files = render_schema2_workflows(&request(generator)?)?;
    Ok(files
        .into_iter()
        .find(|file| file.path == ".github/workflows/qualification.yml")
        .ok_or_else(|| io::Error::other("missing qualification workflow"))?
        .bytes)
}

#[test]
fn stable_sources_mode_runs_only_the_read_only_branch_qualification_graph()
-> Result<(), Box<dyn Error>> {
    let plain = workflow(false)?;
    let workflow = workflow(true)?;
    assert!(!plain.contains("stable-sources-producer-linux"));
    assert!(workflow.contains("default: both"));
    for producer in ["linux", "macos", "intel"] {
        let producer_id = format!("stable-sources-producer-{producer}");
        let stage = section(&workflow, &producer_id)?;
        assert!(stage.contains("stable-source-producer.sh"));
        assert!(stage.contains("include-hidden-files"));
        assert!(stage.contains("steps.upload.outputs.artifact-id"));
        for temperature in ["warm", "cold"] {
            let id = format!("stable-sources-{producer}-{temperature}");
            let body = section(&workflow, &id)?;
            assert!(body.contains("stable-source-consumer.sh"), "{id}");
            assert!(
                body.contains("build-owned-archive-guard.sh --source-cache"),
                "{id}"
            );
            assert!(
                body.contains("Measure source preparation")
                    && body.contains("Measure offline archive guard build"),
                "{id}"
            );
            if temperature == "warm" {
                assert!(
                    body.contains(&format!("needs.{producer_id}.outputs.artifact_id")),
                    "{id}"
                );
                assert!(body.contains("stable-source-admission.py"), "{id}");
                assert!(body.contains("actions/download-artifact@"), "{id}");
            } else {
                assert!(!body.contains("actions/download-artifact@"), "{id}");
                assert!(!body.contains(&format!("needs.{producer_id}")), "{id}");
            }
            for guard in [
                "inputs.mode == 'stable-sources'",
                "github.event_name == 'workflow_dispatch'",
                "github.repository == 'tailrocks/velnor-new'",
                "github.event.repository.default_branch",
            ] {
                assert!(body.contains(guard), "{id} lacks {guard}");
                assert!(stage.contains(guard), "{producer_id} lacks {guard}");
            }
            for privileged in [
                "actions: write",
                "contents: write",
                "attestations: write",
                "id-token: write",
                "cache-mode: write",
                "actions/cache",
                "attest-build-provenance",
            ] {
                assert!(!body.contains(privileged), "{id} has {privileged}");
                assert!(
                    !stage.contains(privileged),
                    "{producer_id} has {privileged}"
                );
            }
        }
    }
    assert!(section(&workflow, "stable-sources-producer-intel")?.contains("runs-on: macos-15"));
    assert!(section(&workflow, "stable-sources-intel-warm")?.contains("runs-on: macos-15-intel"));
    assert!(section(&workflow, "stable-sources-intel-cold")?.contains("runs-on: macos-15-intel"));
    assert!(section(&workflow, "verify-hosted")?.contains("inputs.mode == 'both'"));
    assert!(
        section(&workflow, "mbx-cache-write-hosted")?
            .contains("inputs.mode == 'mbx-cache-roundtrip'")
    );
    Ok(())
}
