use std::collections::BTreeSet;
use std::error::Error;
use std::io::Write;
use std::process::Command;
use std::process::Stdio;

use velnor_actions_contract::RoutingWorkflow;

use crate::schema2::Schema2WorkflowRequest;
use crate::yaml::render_yaml;

use super::super::product_release_family as family;
use super::{Family, ProductRelease, render};

#[path = "schema2_product_release_exec_tests.rs"]
mod exec_tests;
use super::super::product_release_test_pins::test_pins;

fn product(families: &[RoutingWorkflow]) -> Result<ProductRelease, Box<dyn Error>> {
    let request = Schema2WorkflowRequest {
        version: "2.0.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from_iter(families.iter().copied()),
        mbx_qualification: None,
        product_release: Some(test_pins()),
    };
    render(&request)?.ok_or_else(|| "release workflow was not rendered".into())
}

fn all_text(product: &ProductRelease) -> String {
    let mut documents = vec![render_yaml(&product.workflow)];
    documents.extend(
        product
            .family_workflows
            .iter()
            .map(|(_, workflow)| render_yaml(workflow)),
    );
    documents.extend(
        product
            .actions
            .iter()
            .map(|(_, action)| render_yaml(action)),
    );
    documents.join("\n")
}

#[test]
fn empty_release_request_emits_no_workflow() -> Result<(), Box<dyn Error>> {
    let request = Schema2WorkflowRequest {
        version: "2.0.0".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::new(),
        mbx_qualification: None,
        product_release: None,
    };
    assert!(render(&request)?.is_none());
    Ok(())
}

#[test]
fn all_requested_products_share_one_dispatch_only_source_bound_workflow()
-> Result<(), Box<dyn Error>> {
    let product = product(&[
        RoutingWorkflow::ImageRelease,
        RoutingWorkflow::MacosBinaryRelease,
        RoutingWorkflow::GeneratorRelease,
    ])?;
    let rendered = render_yaml(&product.workflow);
    let all = all_text(&product);
    for required in [
        "name: Velnor product releases",
        "workflow_dispatch: {}",
        "cancel-in-progress: false",
        "release-eligibility:",
        "prepare-images:",
        "prepare-binary:",
        "prepare-generator:",
        "release-images:",
        "release-binary:",
        "release-generator:",
        "needs.release-eligibility.outputs.source_sha",
        "needs.release-eligibility.outputs.workflow_authority_sha",
        "needs.release-eligibility.outputs.ci_run_id",
        "needs.release-eligibility.outputs.ci_attempt",
        "needs.prepare-generator.outputs.action",
    ] {
        assert!(rendered.contains(required), "missing {required}");
    }
    for required in [
        "build-images:",
        "build-binary:",
        "build-linux:",
        "build-macos:",
        "build-macos-intel:",
        "candidate-manifest:",
        "qualify-linux:",
        "qualify-macos:",
        "qualify-macos-intel:",
        "attest-images:",
        "attest-binary:",
        "attest-linux:",
        "attest-macos:",
        "attest-macos-intel:",
        "attest-manifest:",
        "publish-images:",
        "publish-binary:",
        "publish-generator:",
        "needs.attest-linux.result == 'success'",
        "needs.attest-macos.result == 'success'",
        "needs.attest-macos-intel.result == 'success'",
        "needs.attest-manifest.result == 'success'",
        "signer-workflow",
        ".github/workflows/product-release-generator.yml",
        "--source-digest",
        "--signer-digest",
        "release-manifest.json",
        "velnor-actions-0.1.4-x86_64-apple-darwin",
        "--target",
        "if $expected_draft then true else .html_url == $html_url end",
    ] {
        assert!(all.contains(required), "missing {required}");
    }
    assert!(!rendered.contains("schedule:"));
    assert!(!rendered.contains("push:"));
    assert!(rendered.contains("workflow_dispatch) ;;"));
    assert!(!rendered.contains("push|schedule|workflow_dispatch"));
    assert!(!all.contains("image-release.yml"));
    assert!(!all.contains("macos-binary-release.yml"));
    assert!(!all.contains("generator-release.yml"));
    assert!(!all.contains("velnor-actions-0.1.0"));
    assert_eq!(product.family_workflows.len(), 3);
    for (path, workflow) in &product.family_workflows {
        assert!(path.starts_with(".github/workflows/product-release-"));
        let body = render_yaml(workflow);
        assert!(body.contains("workflow_call:"), "{path}: {body}");
        assert!(!body.contains("workflow_dispatch:"), "{path}: {body}");
    }
    assert_eq!(
        super::super::generator_release::publication_asset_paths().len(),
        20
    );
    assert!(all.contains("--pattern '"));
    assert!(all.contains("ref: ${{ inputs.source_sha }}"));
    Ok(())
}

#[test]
fn only_requested_families_are_composed() -> Result<(), Box<dyn Error>> {
    let product = product(&[RoutingWorkflow::ImageRelease])?;
    let rendered = render_yaml(&product.workflow);
    assert!(rendered.contains("release-eligibility:"));
    assert!(rendered.contains("prepare-images:"));
    assert!(rendered.contains("release-images:"));
    assert!(!rendered.contains("prepare-binary:"));
    assert!(!rendered.contains("prepare-generator:"));
    assert_eq!(product.family_workflows.len(), 1);
    assert_eq!(
        product.family_workflows[0].0,
        Family::Images.workflow_path()
    );
    Ok(())
}

#[test]
fn generator_publisher_stays_inside_the_typed_release_graph() {
    assert!(family::publish_script(Family::Generator, &test_pins()).is_err());
}

#[test]
fn generator_prepare_uses_the_canonical_inventory_and_manifest_bytes() {
    let generator =
        family::prepare_script(Family::Generator, &test_pins()).expect("generator prepare script");
    for required in [
        "--dir \"$temp_dir/linux-assets\"",
        "--dir \"$temp_dir/macos-assets\"",
        "--dir \"$temp_dir/macos-intel-assets\"",
        "$temp_dir/linux-assets/velnor-actions-0.1.4-x86_64-unknown-linux-gnu.sha256",
        "$temp_dir/macos-assets/velnor-actions-0.1.4-aarch64-apple-darwin.sha256",
        "$temp_dir/macos-intel-assets/velnor-actions-0.1.4-x86_64-apple-darwin.sha256",
        "cmp release-manifest.json manifest-assets/release-manifest.json",
        "readonly fixed_tag='v0.1.4'",
        ".target_commitish == $sha",
        "tag does not resolve to the exact source commit",
    ] {
        assert!(
            generator.contains(required),
            "missing {required}: {generator}"
        );
    }
    assert_eq!(generator.matches("--pattern '").count(), 20);
    assert!(!generator.contains("velnor-actions-0.1.0"));
}

#[test]
fn generator_prepare_rerun_requires_source_target_and_exact_source_tag()
-> Result<(), Box<dyn Error>> {
    let generator = family::prepare_script(Family::Generator, &test_pins())?;
    let expected = extract_prepare_expected_assets(&generator)?;
    let predicate = extract_prepare_metadata_predicate(&generator)?;
    let tag_function = extract_prepare_tag_function(&generator)?;
    let canonical = prepare_release_json(&expected, "0123456789abcdef0123456789abcdef01234567");
    let old_release = prepare_release_json(&expected, "main");

    assert!(run_prepare_metadata_predicate(
        predicate, expected, &canonical
    )?);
    assert!(
        !run_prepare_metadata_predicate(predicate, expected, &old_release)?,
        "existing v0.1.4 with target_commitish=main passed prepare revalidation"
    );
    assert_prepare_tag_target(tag_function)?;
    Ok(())
}

fn extract_prepare_expected_assets(script: &str) -> Result<&str, Box<dyn Error>> {
    let start = script
        .find("readonly prepare_expected_assets='")
        .ok_or("generator prepare asset list is missing")?
        + "readonly prepare_expected_assets='".len();
    let end = script[start..]
        .find('\'')
        .map(|offset| start + offset)
        .ok_or("generator prepare asset list is unterminated")?;
    Ok(&script[start..end])
}

fn extract_prepare_metadata_predicate(script: &str) -> Result<&str, Box<dyn Error>> {
    let start = script
        .find("'.[0] as $release")
        .ok_or("generator prepare metadata predicate is missing")?
        + 1;
    let end = script[start..]
        .find(")' \\\n  <<<\"$matches\"")
        .map(|offset| start + offset + 1)
        .ok_or("generator prepare metadata predicate is unterminated")?;
    Ok(&script[start..end])
}

fn extract_prepare_tag_function(script: &str) -> Result<&str, Box<dyn Error>> {
    let start = script
        .find("assert_tag_target() {")
        .ok_or("generator prepare tag check is missing")?;
    let end = script[start..]
        .find("\n}\n")
        .map(|offset| start + offset + 2)
        .ok_or("generator prepare tag check is unterminated")?;
    Ok(&script[start..end])
}

fn prepare_release_json(expected: &str, target_commitish: &str) -> String {
    let assets = expected
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|name| {
            format!(
                r#"{{"name":{name},"state":"uploaded","size":1,"digest":"sha256:{}"}}"#,
                "a".repeat(64)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"[{{"id":406452151,"tag_name":"v0.1.4","target_commitish":"{target_commitish}","url":"https://api.github.com/repos/tailrocks/velnor-new/releases/406452151","html_url":"https://github.com/tailrocks/velnor-new/releases/tag/v0.1.4","draft":false,"prerelease":false,"immutable":true,"assets":[{assets}]}}]"#
    )
}

fn run_prepare_metadata_predicate(
    predicate: &str,
    expected: &str,
    release: &str,
) -> Result<bool, Box<dyn Error>> {
    let mut child = Command::new("jq")
        .args([
            "-e",
            "--arg",
            "tag",
            "v0.1.4",
            "--arg",
            "sha",
            "0123456789abcdef0123456789abcdef01234567",
            "--argjson",
            "expected",
            expected,
            predicate,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("could not write generator release fixture")?
        .write_all(release.as_bytes())?;
    Ok(child.wait_with_output()?.status.success())
}

fn assert_prepare_tag_target(function: &str) -> Result<(), Box<dyn Error>> {
    let script = format!(
        "repository='tailrocks/velnor-new'\nsource_sha='0123456789abcdef0123456789abcdef01234567'\nprepare_tag='v0.1.4'\ngh() {{ test \"$*\" = 'api repos/tailrocks/velnor-new/git/ref/tags/v0.1.4' || return 1; printf '%s\\n' \"$GH_TAG_FIXTURE\"; }}\n{function}\nassert_tag_target\n"
    );
    let output = Command::new("bash")
        .args(["-euo", "pipefail", "-c", &script])
        .env(
            "GH_TAG_FIXTURE",
            r#"{"object":{"type":"commit","sha":"0123456789abcdef0123456789abcdef01234567"}}"#,
        )
        .output()?;
    assert!(
        output.status.success(),
        "exact generator source tag failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
fn non_generator_publication_keeps_immutable_tag_and_attestation_checks() {
    let pins = test_pins();
    let images = family::prepare_script(Family::Images, &pins).expect("image prepare script");
    assert!(images.contains("--dir \"$temp_dir/assets\""));
    assert!(images.contains("velnor-runner-linux-amd64.tar"));
    assert!(images.contains("velnor-dind-linux-amd64.tar"));

    let publisher = family::publish_script(Family::Binary, &pins).expect("binary publish script");
    assert!(publisher.contains("assets/velnor-host"));
    assert!(publisher.contains("velnor-host binary built from $release_source_sha."));
    assert!(publisher.contains("--source-ref refs/heads/main"));
    assert!(!publisher.contains("--clobber"));
    assert!(publisher.contains("--latest=false"));
    assert!(publisher.contains("--draft"));
    assert!(publisher.contains("--verify-tag"));
    let create_ref = publisher.find("git/refs").expect("explicit tag creation");
    let create_release = publisher
        .find("family_gh release create")
        .expect("draft release creation");
    assert!(create_ref < create_release, "{publisher}");
}

#[test]
fn generated_family_shell_scripts_parse_without_execution() -> Result<(), Box<dyn Error>> {
    for family in [Family::Images, Family::Binary] {
        for script in [
            family::prepare_script(family, &test_pins())?,
            family::publish_script(family, &test_pins())?,
        ] {
            let result = Command::new("bash")
                .args(["-n", "-c"])
                .arg(script)
                .output()?;
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
    let generator = family::prepare_script(Family::Generator, &test_pins())?;
    let result = Command::new("bash")
        .args(["-n", "-c"])
        .arg(generator)
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}
