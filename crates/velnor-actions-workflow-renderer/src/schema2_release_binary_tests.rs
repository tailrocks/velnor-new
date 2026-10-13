use std::collections::BTreeSet;
use std::error::Error;
use std::process::Command;

use velnor_actions_contract::RoutingWorkflow;

use crate::schema2::{Schema2WorkflowRequest, product_release_test_pins::test_pins};
use crate::yaml::Yaml;
use crate::yaml::render_yaml;

use super::macos_binary_release;

fn binary_request() -> Result<Schema2WorkflowRequest, Box<dyn Error>> {
    Ok(Schema2WorkflowRequest {
        version: "2.0.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::MacosBinaryRelease]),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(test_pins()),
    })
}

fn rendered_binary_workflow() -> Result<String, Box<dyn Error>> {
    Ok(render_yaml(&macos_binary_release(&binary_request()?)?))
}

fn map_value<'a>(fields: &'a [(String, Yaml)], key: &str) -> Option<&'a Yaml> {
    fields
        .iter()
        .find_map(|(name, value)| (name == key).then_some(value))
}

fn assert_generated_run_scripts_parse(workflow: &Yaml) -> Result<(), Box<dyn Error>> {
    let Yaml::Map(fields) = workflow else {
        return Err("binary workflow is not a map".into());
    };
    let Some(Yaml::Map(jobs)) = map_value(fields, "jobs") else {
        return Err("binary workflow has no jobs map".into());
    };
    let mut scripts = 0;
    for (job_name, job) in jobs {
        let Yaml::Map(job_fields) = job else {
            return Err(format!("{job_name} job is not a map").into());
        };
        let Some(Yaml::Seq(steps)) = map_value(job_fields, "steps") else {
            continue;
        };
        for step in steps {
            let Yaml::Map(step_fields) = step else {
                continue;
            };
            let Some(Yaml::Str(script)) = map_value(step_fields, "run") else {
                continue;
            };
            scripts += 1;
            let output = Command::new("bash").args(["-n", "-c", script]).output()?;
            if !output.status.success() {
                return Err(format!(
                    "{job_name} run script does not parse: {}",
                    String::from_utf8_lossy(&output.stderr)
                )
                .into());
            }
        }
    }
    if scripts == 0 {
        return Err("binary workflow contains no run scripts".into());
    }
    Ok(())
}

#[test]
fn helper_is_built_before_host_and_its_digest_is_bound_into_host_build()
-> Result<(), Box<dyn Error>> {
    let workflow = rendered_binary_workflow()?;
    let helper = workflow
        .find("Build attestation helper")
        .ok_or("helper build step is missing")?;
    let host = workflow
        .find("Build velnor-host")
        .ok_or("host build step is missing")?;
    let manifest = workflow
        .find("Write binary manifest and checksums")
        .ok_or("manifest step is missing")?;
    assert!(helper < host && host < manifest, "{workflow}");
    assert!(workflow.contains("-p velnor-runner-attestation"));
    assert!(workflow.contains("--bin velnor-runner-attestation-helper"));
    assert!(workflow.contains("steps.helper.outputs.sha256"));
    assert!(workflow.contains("VELNOR_ATTESTATION_HELPER_SHA256"));
    assert!(workflow.contains("[[ \\\"$authority_sha\\\" == \\\"$source_sha\\\" ]]"));
    assert!(workflow.contains("\\\"workflow_authority_sha\\\":\\\"%s\\\""));
    Ok(())
}

#[test]
fn binary_family_attests_and_publishes_the_exact_four_asset_contract() -> Result<(), Box<dyn Error>>
{
    let expected = [
        "velnor-host",
        "velnor-runner-attestation-helper",
        "BINARY_RELEASE_MANIFEST.json",
        "SHA256SUMS",
    ];
    assert_eq!(
        crate::schema2::product_release_family::Family::Binary.asset_names(),
        expected.map(str::to_owned)
    );

    let workflow = rendered_binary_workflow()?;
    for asset in expected {
        assert!(workflow.contains(asset), "missing {asset}: {workflow}");
    }
    assert!(workflow.contains(
        "shasum -a 256 velnor-host velnor-runner-attestation-helper BINARY_RELEASE_MANIFEST.json > SHA256SUMS"
    ));
    assert!(workflow.contains("attest-binary"));
    assert!(workflow.contains("publish-binary"));
    Ok(())
}

#[test]
fn manifest_is_canonical_source_bound_and_excludes_self_digest() -> Result<(), Box<dyn Error>> {
    let workflow = rendered_binary_workflow()?;
    let source_ref = workflow
        .find("\\\"source_ref\\\":\\\"refs/heads/main\\\"")
        .ok_or("source ref is not fixed")?;
    let source_commit = workflow
        .find("\\\"source_commit\\\":\\\"%s\\\"")
        .ok_or("manifest source commit is missing")?;
    let authority = workflow
        .find("\\\"workflow_authority_sha\\\":\\\"%s\\\"")
        .ok_or("manifest authority SHA is missing")?;
    let host = workflow
        .find("\\\"host\\\":{\\\"name\\\":\\\"velnor-host\\\"")
        .ok_or("host manifest entry is missing")?;
    let helper = workflow
        .find("\\\"attestation_helper\\\":{\\\"name\\\":\\\"velnor-runner-attestation-helper\\\"")
        .ok_or("helper manifest entry is missing")?;
    assert!(source_ref < source_commit && source_commit < authority);
    assert!(authority < host && host < helper);
    assert!(workflow.contains("\\\"schema_version\\\":1"));
    assert!(workflow.contains("\\\"target\\\":\\\"aarch64-apple-darwin\\\""));
    assert!(
        workflow
            .contains("\\\"oidc_issuer\\\":\\\"https://token.actions.githubusercontent.com\\\"")
    );
    assert!(
        workflow.contains(
            "\\\"signer_workflow\\\":\\\".github/workflows/product-release-binary.yml\\\""
        )
    );
    assert!(workflow.contains(
        "\\\"certificate_identity\\\":\\\"https://github.com/tailrocks/velnor-new/.github/workflows/product-release-binary.yml@refs/heads/main\\\""
    ));
    assert!(!workflow.contains("manifest_sha256"));
    Ok(())
}

#[test]
fn generated_binary_run_scripts_parse_as_bash() -> Result<(), Box<dyn Error>> {
    let document = macos_binary_release(&binary_request()?)?;
    assert_generated_run_scripts_parse(&document)
}
