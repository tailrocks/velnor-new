// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    process::Command,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use velnor_actions_actionlint::actions::MISE_ACTION_SHA;
use velnor_actions_contract::{
    RoutingWorkflow, Step, StepKind, config::MacRunnerQualificationConfig,
    config::jactionlint_verifier::jactionlint_verifier_script,
};
use velnor_actions_mise::MISE_VERSION;
use velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude;
use velnor_actions_workflow_renderer::{
    ProductReleasePins, Schema2WorkflowRequest, VERIFY_PATH, render_schema2_workflows,
    setup::{MISE_BINARY_SHA256_LINUX_X64, MiseSetup},
};

#[path = "runner_draft_publication_argv.rs"]
mod runner_draft_publication_argv;
use runner_draft_publication_argv::*;

const HEX: &[u8; 16] = b"0123456789abcdef";

fn fixture_step() -> Step {
    Step {
        name: "fixture".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

fn pins() -> ProductReleasePins {
    let setup = MiseSetup {
        uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    };
    ProductReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup.clone(),
        macos_x86_64_setup: setup,
        install_gate_tools_argv: argv(&["mise", "install", "gh@2.103.2"]),
        install_build_tools_argv: argv(&["mise", "install", "rust@1.91.0", "mr-boxington@1.0.0"]),
        install_qualify_tools_argv: argv(&["mise", "install", "opentofu@1.13.2"]),
        qualify_mbx_action_steps: [fixture_step(), fixture_step(), fixture_step()],
        install_runner_build_tools_argv: install_runner_build_tools_argv(),
        install_gh_argv: argv(&["mise", "install", "gh@2.103.2"]),
        build_argv: argv(&[
            "mbx",
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ]),
        intel_build_argv: argv(&[
            "mbx",
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
            "--target",
            "x86_64-apple-darwin",
        ]),
        install_intel_target_argv: argv(&["rustup", "target", "add", "x86_64-apple-darwin"]),
        install_resource_probe_target_argv: install_resource_probe_target_argv(),
        runner_build_argv: runner_build_argv(),
        attestation_helper_build_argv: attestation_helper_build_argv(),
        resource_probe_build_argv: resource_probe_build_argv(),
        install_resource_probe_release_tools_argv: install_resource_probe_release_tools_argv(),
        resource_probe_release_build_argv: resource_probe_release_build_argv(),
        resource_probe_release_test_argv: resource_probe_release_test_argv(),
        lint_argv: argv(&["bash", "-c", "exit 0"]),
        verify_lint_argv: verify_lint_argv(),
        gh_argv: argv(&["gh"]),
        rust_version: "1.91.0".to_owned(),
        verify_rust_argv: verify_rust_argv(),
        mr_boxington_version: "1.0.0".to_owned(),
        gh_version: "2.103.2".to_owned(),
    }
}

fn rendered() -> Result<Vec<velnor_actions_workflow_renderer::RenderedFile>, Box<dyn Error>> {
    let mut request = Schema2WorkflowRequest {
        runner_image_family: Some(MacRunnerQualificationConfig {
            runner_id: 2,
            companion_workflow_path: ".github/workflows/qualify-runner-family.yml".to_owned(),
            companion_workflow_ref: "refs/heads/main".to_owned(),
            companion_workflow_sha: "c".repeat(40),
            docker_socket: "unix:///tmp/fixture.sock".to_owned(),
            state_dir: "/tmp/fixture-state".to_owned(),
            sdk: velnor_actions_contract::config::MacSdkSelection::Stable {
                version: "1.2.3".to_owned(),
                source_sha: "a".repeat(40),
                manifest_sha256: "b".repeat(64),
            },
        }),
        version: "0.1.78".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::from([RoutingWorkflow::ImageRelease]),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(pins()),
    };
    request
        .runner_image_family
        .as_mut()
        .ok_or("family missing")?;
    Ok(render_schema2_workflows(&request)?)
}

fn image_workflow(
    files: &[velnor_actions_workflow_renderer::RenderedFile],
) -> Result<serde_json::Value, Box<dyn Error>> {
    let file = files
        .iter()
        .find(|file| file.path == ".github/workflows/product-release-images.yml")
        .ok_or("runner release workflow missing")?;
    Ok(serde_saphyr::from_str(&file.bytes)?)
}

fn digest(bytes: &[u8]) -> String {
    let mut result = String::from("sha256:");
    for byte in Sha256::digest(bytes) {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 15)]));
    }
    result
}

fn runner_draft_gate(publish_script: &str) -> Result<String, Box<dyn Error>> {
    let start = publish_script
        .find("for digest in \"$VELNOR_MANIFEST_DIGEST\"")
        .ok_or("draft digest gate missing")?;
    let end = publish_script[start..]
        .find("check_eligibility_identity")
        .ok_or("post-gate identity recheck missing")?
        + start;
    Ok(publish_script[start..end].to_owned())
}

fn materialized_publish_script(document: &serde_json::Value) -> Result<String, Box<dyn Error>> {
    let steps = document["jobs"]["release-images"]["steps"]
        .as_array()
        .ok_or("release steps missing")?;
    let mut chunks = Vec::new();
    for step in steps {
        let Some(run) = step["run"].as_str() else {
            continue;
        };
        chunks.extend(run.lines().filter_map(|line| {
            let marker = "printf '%s' '";
            let (_, suffix) = line.split_once(marker)?;
            let (chunk, rest) = suffix.split_once('\'')?;
            rest.contains("publish.sh.b64").then_some(chunk)
        }));
    }
    if chunks.is_empty() {
        return Err("publisher source chunks missing".into());
    }
    let encoded = chunks.concat();
    Ok(String::from_utf8(STANDARD.decode(encoded)?)?)
}

fn run_draft_gate(
    gate: &str,
    server_release: &serde_json::Value,
    values: &[String],
) -> Result<(bool, String, String), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let response = directory.path().join("release.json");
    let edit_log = directory.path().join("edit.log");
    let script = directory.path().join("gate.sh");
    fs::write(&response, serde_json::to_vec(server_release)?)?;
    fs::write(
        &script,
        format!(
            "set -euo pipefail\nrelease_repository=tailrocks/velnor-new\nrelease_tag=runner-{}\nfamily_fail() {{ printf '%s\\n' \"$1\" >&2; exit 1; }}\nfamily_gh() {{ if [[ \"$1\" == api ]]; then cat \"$SERVER_RELEASE\"; elif [[ \"$1\" == release && \"$2\" == edit ]]; then printf '%s\\n' \"$*\" >> \"$EDIT_LOG\"; else exit 98; fi; }}\n{}\nfamily_gh release edit \"$release_tag\" --draft=false\n",
            "a".repeat(40),
            gate
        ),
    )?;
    let output = Command::new("/bin/bash")
        .arg(&script)
        .env("SERVER_RELEASE", response)
        .env("EDIT_LOG", &edit_log)
        .env("VELNOR_MANIFEST_DIGEST", &values[0])
        .env("VELNOR_CHECKSUMS_DIGEST", &values[1])
        .env("VELNOR_LICENSE_DIGEST", &values[2])
        .env("VELNOR_NOTICE_DIGEST", &values[3])
        .output()?;
    let edits = fs::read_to_string(&edit_log).unwrap_or_default();
    Ok((
        output.status.success(),
        edits,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

fn verify_attestation_job(
    jobs: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    let verifier = &jobs["verify-mac-proof"];
    assert_eq!(verifier["timeout-minutes"], 140);
    assert_eq!(verifier["permissions"]["actions"], "read");
    assert_eq!(verifier["permissions"]["contents"], "read");
    assert!(verifier["permissions"].get("id-token").is_none());
    assert!(verifier["permissions"].get("attestations").is_none());
    assert!(verifier["permissions"].get("packages").is_none());
    assert!(
        verifier["steps"]
            .as_array()
            .ok_or("verifier steps missing")?
            .iter()
            .any(|step| step["id"] == "verify_mac")
    );
    let attest = &jobs["attest-images"];
    assert!(
        attest["needs"]
            .as_array()
            .ok_or("attest needs missing")?
            .iter()
            .any(|job| job == "verify-mac-proof")
    );
    let steps = attest["steps"].as_array().ok_or("attest steps missing")?;
    assert_eq!(steps.len(), 4);
    let subjects = [
        ("RUNNER_IMAGE_MANIFEST.json", "manifest_digest"),
        ("SHA256SUMS", "checksums_digest"),
        ("LICENSE", "license_digest"),
        ("NOTICE", "notice_digest"),
    ];
    for (step, (name, digest_key)) in steps.iter().zip(subjects) {
        assert_eq!(
            step["uses"],
            velnor_actions_actionlint::ATTEST_BUILD_PROVENANCE_USES
        );
        assert_eq!(step["with"]["subject-name"], name);
        assert_eq!(
            step["with"]["subject-digest"],
            format!("${{{{ needs.verify-mac-proof.outputs.{digest_key} }}}}")
        );
        assert!(step["with"].get("subject-path").is_none());
        assert!(step.get("run").is_none());
    }
    Ok(())
}

fn release_publish_script(
    jobs: &serde_json::Map<String, serde_json::Value>,
) -> Result<String, Box<dyn Error>> {
    let release = &jobs["release-images"];
    assert!(
        release["needs"]
            .as_array()
            .ok_or("release needs missing")?
            .iter()
            .any(|need| need == "attest-images")
    );
    let publish = release["steps"]
        .as_array()
        .ok_or("release steps missing")?
        .iter()
        .find(|step| step["name"] == "Publish immutable source release")
        .ok_or("release publisher missing")?;
    for (environment, output) in [
        ("VELNOR_MANIFEST_DIGEST", "manifest_digest"),
        ("VELNOR_CHECKSUMS_DIGEST", "checksums_digest"),
        ("VELNOR_LICENSE_DIGEST", "license_digest"),
        ("VELNOR_NOTICE_DIGEST", "notice_digest"),
    ] {
        assert_eq!(
            publish["env"][environment],
            format!("${{{{ needs.attest-images.outputs.{output} }}}}")
        );
    }
    materialized_publish_script(&serde_json::json!({"jobs":jobs}))
}

fn verify_digest_gate(script: &str) -> Result<(), Box<dyn Error>> {
    let upload = script
        .find("release upload")
        .ok_or("asset upload missing")?;
    let gate = script
        .find("for digest in \"$VELNOR_MANIFEST_DIGEST\"")
        .ok_or("digest gate missing")?;
    let draft_edit = script
        .find("--draft=false")
        .ok_or("draft transition missing")?;
    assert!(upload < gate && gate < draft_edit);
    let gate = runner_draft_gate(script)?;
    let original = [
        b"manifest-v1".as_slice(),
        b"checksums-v1",
        b"license-v1",
        b"notice-v1",
    ];
    let expected: Vec<String> = original.iter().map(|bytes| digest(bytes)).collect();
    let release_json = |digests: &[String]| {
        serde_json::json!({
            "tag_name":format!("runner-{}", "a".repeat(40)),
            "draft":true,
            "assets":[
                {"name":"RUNNER_IMAGE_MANIFEST.json","digest":digests[0]},
                {"name":"SHA256SUMS","digest":digests[1]},
                {"name":"LICENSE","digest":digests[2]},
                {"name":"NOTICE","digest":digests[3]}
            ]
        })
    };
    let (accepted, edits, error) = run_draft_gate(&gate, &release_json(&expected), &expected)?;
    assert!(accepted, "matching server release rejected: {error}");
    assert!(edits.contains("--draft=false"));
    let mut uploaded = expected.clone();
    uploaded[0] = digest(b"manifest-mutated-after-attestation");
    let (accepted, edits, _) = run_draft_gate(&gate, &release_json(&uploaded), &expected)?;
    assert!(!accepted);
    assert_eq!(edits, "");
    Ok(())
}

#[test]
fn runner_release_attests_verified_digests_and_keeps_mutated_uploads_draft()
-> Result<(), Box<dyn Error>> {
    let document = image_workflow(&rendered()?)?;
    let jobs = document["jobs"].as_object().ok_or("jobs missing")?;
    verify_attestation_job(jobs)?;
    verify_digest_gate(&release_publish_script(jobs)?)
}

#[path = "runner_publication_context.rs"]
mod runner_publication_context;
