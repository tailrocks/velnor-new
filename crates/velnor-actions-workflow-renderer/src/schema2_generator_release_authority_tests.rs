use super::{GeneratorRelease, generator_release};
use crate::schema2::{Schema2WorkflowRequest, product_release_test_pins};
use crate::yaml::{Yaml, render_yaml};
use std::collections::BTreeSet;

const PRIVILEGED: [&str; 5] = [
    "attest-linux",
    "attest-macos",
    "attest-macos-intel",
    "attest-manifest",
    "publish-generator",
];

#[test]
fn privileged_generator_jobs_are_source_free_and_use_native_steps() {
    let request = Schema2WorkflowRequest {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        hosted_label: "ubuntu-24.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set().expect("scale set"),
        routing: crate::workflow_routing::WorkflowRoutingPolicy::hosted_only(),
        workflows: BTreeSet::new(),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(product_release_test_pins::test_pins()),
    };
    let generated = generator_release(&request).expect("generator workflow renders");
    assert_privileged_jobs_are_source_free(&generated);
}

fn assert_privileged_jobs_are_source_free(generated: &GeneratorRelease) {
    let rendered = render_yaml(&generated.workflow);
    for id in PRIVILEGED {
        assert_one_privileged_job_is_source_free(&rendered, id);
    }
    assert_build_artifact_ids_flow(&rendered);
    assert_retired_privileged_actions_absent(generated);
}

fn assert_one_privileged_job_is_source_free(workflow: &str, id: &str) {
    let section = job_section(workflow, id);
    for forbidden in [
        "actions/checkout@",
        "uses: ./.github/actions/",
        "scripts/",
        "cargo build",
        "cargo run",
        "Resolve project tool versions",
        "VELNOR_TOOL_",
    ] {
        assert!(!section.contains(forbidden), "{id} contains {forbidden}");
    }
    assert!(
        section.contains(
            "VELNOR_RELEASE_GH_VERSION: ${{ needs.candidate-manifest.outputs.gh_version }}"
        ),
        "{id} must bind the validated GitHub CLI version"
    );
    assert!(
        section.contains(
            r#"--no-config --no-env --no-hooks install 'gh@'\"$VELNOR_RELEASE_GH_VERSION\""#
        ),
        "{id} installs the source-bound GH version"
    );
    assert!(
        section.contains(
            r#"--no-config --no-env --no-hooks exec 'gh@'\"$VELNOR_RELEASE_GH_VERSION\" -- gh"#
        ),
        "{id} executes the source-bound GH version"
    );
    assert!(section.contains("candidate-manifest"), "{id}");
    let validation = section
        .find("Validate source-bound GitHub CLI version")
        .expect("GH version validation step");
    let install = section
        .find("Install source-bound GitHub CLI")
        .expect("GH version install step");
    assert!(validation < install, "{id} validates before installation");
}

fn assert_build_artifact_ids_flow(workflow: &str) {
    for (consumer, producer) in [
        ("attest-linux", "build-linux"),
        ("attest-macos", "build-macos"),
        ("attest-macos-intel", "build-macos-intel"),
    ] {
        let section = job_section(workflow, consumer);
        let artifact_id = format!("needs.{producer}.outputs.artifact_id");
        assert_download_step_uses_artifact_id(&section, &artifact_id);
        assert!(
            section.contains("artifact-ids:") && section.contains(&artifact_id),
            "{consumer} must download {producer}'s immutable artifact ID"
        );
    }
    let manifest = job_section(workflow, "attest-manifest");
    let publisher = job_section(workflow, "publish-generator");
    for producer in ["build-linux", "build-macos", "build-macos-intel"] {
        let artifact_id = format!("needs.{producer}.outputs.artifact_id");
        assert!(
            manifest.contains("artifact-ids:") && manifest.contains(&artifact_id),
            "manifest attester misses {producer}"
        );
        assert!(
            publisher.contains("artifact-ids:") && publisher.contains(&artifact_id),
            "publisher misses {producer}"
        );
    }
}

fn assert_retired_privileged_actions_absent(generated: &GeneratorRelease) {
    for (path, _) in &generated.actions {
        assert!(
            ![
                "generator-release-attest-linux",
                "generator-release-attest-macos",
                "generator-release-attest-macos-intel",
                "generator-release-attest-manifest",
                "generator-release-publish",
            ]
            .iter()
            .any(|retired| path.contains(retired)),
            "retired privileged composite emitted: {path}"
        );
    }
}

fn assert_download_step_uses_artifact_id(section: &str, artifact_id: &str) {
    let start = section
        .find("name: Download built asset files")
        .expect("attester has its raw artifact download step");
    let following = &section[start..];
    let end = following.find("\n      - name:").unwrap_or(following.len());
    let download = &following[..end];
    assert!(
        download.contains("uses: actions/download-artifact@"),
        "{download}"
    );
    assert!(
        download.contains(&format!("artifact-ids: ${{{{ {artifact_id} }}}}")),
        "download step must select the producer artifact ID: {download}"
    );
}

fn job_section(workflow: &str, id: &str) -> String {
    let marker = format!("  {id}:");
    let start = workflow
        .find(&marker)
        .unwrap_or_else(|| panic!("missing job {id}"));
    let remainder = &workflow[start + marker.len()..];
    let end = remainder
        .match_indices("\n  ")
        .find_map(|(index, _)| {
            let following = remainder[index + 3..].chars().next()?;
            (!following.is_whitespace()).then_some(index)
        })
        .unwrap_or(remainder.len());
    remainder[..end].to_owned()
}

#[test]
fn candidate_manifest_exports_only_exact_release_toolchain_values() {
    let request = Schema2WorkflowRequest {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        hosted_label: "ubuntu-24.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set().expect("scale set"),
        routing: crate::workflow_routing::WorkflowRoutingPolicy::hosted_only(),
        workflows: BTreeSet::new(),
        mbx_qualification: None,
        mise_pin_qualification: None,
        product_release: Some(product_release_test_pins::test_pins()),
    };
    let generated = generator_release(&request).expect("generator workflow renders");
    let rendered = render_yaml(&generated.workflow);
    let candidate = job_section(&rendered, "candidate-manifest");
    assert!(candidate.contains("rust_version:"));
    assert!(candidate.contains("steps.authority_toolchain.outputs.rust_version"));
    assert!(candidate.contains("mr_boxington_version:"));
    assert!(candidate.contains("steps.authority_toolchain.outputs.mr_boxington_version"));
    assert!(candidate.contains("gh_version:"));
    assert!(candidate.contains("steps.authority_toolchain.outputs.gh_version"));
    assert!(
        candidate.contains(r"exact_semver='^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)$'")
    );
}

#[test]
fn source_bound_gh_version_is_quoted_at_runtime_and_validated_before_mise() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("velnor-authority-mise-{unique}"));
    fs::create_dir(&dir).expect("temporary mock bin directory");
    let log = dir.join("calls");
    let mock = dir.join("mise");
    fs::write(
        &mock,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$MOCK_MISE_LOG\"\n",
    )
    .expect("mock executable");
    let mut permissions = fs::metadata(&mock).expect("mock metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&mock, permissions).expect("make mock executable");

    let validation = super::jobs::authority_gh_validation_step();
    let Yaml::Map(fields) = validation else {
        panic!("validation step map")
    };
    let validation = fields
        .iter()
        .find_map(|(k, v)| (k == "run").then_some(v))
        .expect("validation run field");
    let Yaml::Str(validation) = validation else {
        panic!("validation run string")
    };
    let install = crate::commands::join_argv_for_run(&super::jobs::authority_gh_install_argv())
        .expect("runtime version expansion is valid argv");
    assert!(
        install.contains("'gh@'\"$VELNOR_RELEASE_GH_VERSION\""),
        "{install}"
    );
    let execute = crate::commands::join_argv_for_run(&super::jobs::authority_gh_argv())
        .expect("runtime exec version expansion is valid argv");
    assert!(
        execute.contains("'gh@'\"$VELNOR_RELEASE_GH_VERSION\""),
        "{execute}"
    );

    for (version, succeeds, expected_call) in [
        (
            "2.103.2",
            true,
            Some(
                "--no-config --no-env --no-hooks install gh@2.103.2\n--no-config --no-env --no-hooks exec gh@2.103.2 -- gh",
            ),
        ),
        ("01.2.3", false, None),
        ("2.3.4;touch /tmp/velnor-should-not-run", false, None),
    ] {
        let output = Command::new("/bin/bash")
            .arg("-c")
            .arg(format!("{validation}\n{install}\n{execute}"))
            .env("VELNOR_RELEASE_GH_VERSION", version)
            .env("MOCK_MISE_LOG", &log)
            .env("PATH", &dir)
            .output()
            .expect("run mock Mise command");
        assert_eq!(output.status.success(), succeeds, "version {version}");
        let calls = fs::read_to_string(&log).unwrap_or_default();
        if let Some(expected) = expected_call {
            assert_eq!(calls.trim(), expected);
            fs::remove_file(&log).expect("reset mock log");
        } else {
            assert!(calls.is_empty(), "invalid version reached Mise: {calls}");
        }
    }
    fs::remove_dir_all(dir).expect("remove mock executable");
}

#[test]
fn privileged_publisher_keeps_api_eligibility_rechecks_without_local_git() {
    let script = super::manifest::publish_script();
    assert!(!script.contains("git rev-parse HEAD"));
    assert!(script.contains("assert_current_tip"));
    assert!(script.contains("latest exact-source CI run did not succeed"));
    assert!(script.contains("assert_required_job"));
    assert_eq!(script.matches("\nrelease_eligibility\n").count(), 2);
}
