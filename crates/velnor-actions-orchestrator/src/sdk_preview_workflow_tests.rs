// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use std::{collections::BTreeMap, error::Error};
use velnor_actions_contract::{
    GeneratorLock, NonInputs, ReleaseTarget, WorkflowPolicy, config::VelnorConfig,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::schema2::{ConsumerReleaseSpec, render_consumer_preview};

fn spec() -> Result<ConsumerReleaseSpec, Box<dyn Error>> {
    let mut config: VelnorConfig = crate::config::load_config_from_text(
        r#"schema = 2
[workflow]
policy = "consumer-v1"
[execution]
default_profile = "hosted"
hosted_profile = "hosted"
scale_set_profile = "local"
[execution.profiles.hosted]
kind = "github-hosted"
label = "ubuntu-26.04"
platform = "linux/amd64"
[execution.profiles.local]
kind = "github-scale-set"
name = "ubuntu-26.04-scale-set"
labels = ["velnor", "ubuntu-26.04-scale-set"]
platform = "linux/amd64"
"#,
    )?;
    config.workflow.policy = WorkflowPolicy::VelnorRepositoryV1;
    let release = crate::sdk_preview_configuration::release(&config)?;
    let plan = super::tests::task()?.request.plan;
    let policy: velnor_actions_contract::PackageReleasePolicy = serde_json::from_value(
        serde_json::json!({
            "schema":"velnor.package-release-policy.v1","channel":"preview","repository":"tailrocks/velnor-new","default_branch":"main",
            "version":{"version_package":"velnor-actions-cli","bindings":[{"package":"velnor-actions-cli","manifest_path":"crates/velnor-actions-cli/Cargo.toml","workspace_manifest_path":"Cargo.toml"},{"package":"velnor-archive-guard","manifest_path":"crates/velnor-archive-guard/Cargo.toml","workspace_manifest_path":"Cargo.toml"}]},"products":plan.products,"companions":[]
        }),
    )?;
    let lock: GeneratorLock = velnor_actions_mise::catalog::lock::parse_generator_lock(
        include_str!("../../../.velnor/generator.lock"),
    )?;
    let mut acquires = BTreeMap::new();
    let mut setups = BTreeMap::new();
    for target in ReleaseTarget::ALL {
        acquires.insert(
            target,
            crate::pins::lock_acquire_for_target(
                &lock,
                target,
                &format!(
                    "$RUNNER_TEMP/velnor/bin/velnor-actions-{}",
                    env!("CARGO_PKG_VERSION")
                ),
            )?,
        );
        setups.insert(
            target,
            crate::pins::resolve_mise_setup_for_release_target(&config, target)?,
        );
    }
    Ok(ConsumerReleaseSpec {
        sdk_source_bootstrap: true,
        generator_version: env!("CARGO_PKG_VERSION").into(),
        checkout_lfs: false,
        policy,
        producer_runner: release.producer.runner,
        producer_timeout_minutes: release.producer.timeout_minutes,
        publication_timeout_minutes: release.publication_timeout_minutes,
        linux_acquire: acquires[&ReleaseTarget::LinuxX86_64].clone(),
        producer_acquire: acquires[&ReleaseTarget::LinuxX86_64].clone(),
        linux_setup: setups[&ReleaseTarget::LinuxX86_64].clone(),
        producer_setup: setups[&ReleaseTarget::LinuxX86_64].clone(),
        producer_acquire_by_target: acquires,
        producer_setup_by_target: setups,
        install_gh_argv: vec!["mise".into(), "install".into(), "gh@2.103.3".into()],
        install_rust_argv: vec!["mise".into(), "install".into(), "rust@1.99.1".into()],
        verify_rust_argv: crate::workflow::verify_rust_argv(&ToolCatalog::pinned()),
        producer_source_prep: vec![],
        producer_cargo_env: BTreeMap::new(),
        gh_argv: vec!["gh".into()],
        homebrew_tap: None,
        stable: None,
        non_inputs: NonInputs::default(),
    })
}

fn workflow(spec: &ConsumerReleaseSpec) -> Result<Value, Box<dyn Error>> {
    let rendered = render_consumer_preview(spec)?;
    assert_eq!(rendered.path, ".github/workflows/binary-preview.yml");
    Ok(serde_saphyr::from_str(&rendered.bytes)?)
}

#[test]
fn sdk_preview_keeps_locked_privileged_tools_and_uses_source_cli_only_in_read_only_jobs()
-> Result<(), Box<dyn Error>> {
    let spec = spec()?;
    let document = workflow(&spec)?;
    assert!(document["on"].get("workflow_run").is_some());
    assert!(document["on"].get("workflow_dispatch").is_none());
    assert!(document["jobs"].get("preview-recovery").is_none());
    let lock: GeneratorLock = velnor_actions_mise::catalog::lock::parse_generator_lock(
        include_str!("../../../.velnor/generator.lock"),
    )?;
    let linux = lock
        .binary_for_target(ReleaseTarget::LinuxX86_64.triple())
        .ok_or("missing Linux x86_64 locked binary")?;
    for id in ["preview-signer", "preview-publisher"] {
        let job = &document["jobs"][id];
        let text = serde_json::to_string(job)?;
        assert!(text.contains(&linux.artifact));
        assert!(text.contains(&linux.sha256));
        assert!(!text.contains("actions/checkout@"));
        assert!(!text.contains("velnor-actions-source"));
        assert!(!text.contains(".velnor/release-manifest.json"));
    }
    for id in ["preview-eligibility", "preview-payload", "preview-assemble"] {
        let job = &document["jobs"][id];
        assert_eq!(job["permissions"]["contents"], "read");
        assert!(job["permissions"].get("id-token").is_none());
        let steps = job["steps"]
            .as_array()
            .ok_or("job steps are not an array")?;
        assert!(
            steps
                .iter()
                .any(|s| s["name"] == "Build exact-source SDK for read-only preview production")
        );
        for step in steps {
            if ["finalize-preview-source-v1", "execute-release-producer-v1"].contains(
                &step["env"]["VELNOR_INTERNAL_OP"]
                    .as_str()
                    .unwrap_or_default(),
            ) {
                assert!(
                    step["run"]
                        .as_str()
                        .ok_or("step run is not a string")?
                        .contains("velnor-actions-source")
                );
            }
        }
    }
    let steps = document["jobs"]["preview-payload"]["steps"]
        .as_array()
        .ok_or("preview payload steps are not an array")?;
    let gate = steps
        .iter()
        .position(|s| s["name"] == "Verify exact SDK compiler input inventory")
        .ok_or("SDK compiler input gate is missing")?;
    let upload = steps
        .iter()
        .position(|s| {
            s["uses"]
                .as_str()
                .is_some_and(|u| u.starts_with("actions/upload-artifact@"))
        })
        .ok_or("artifact upload step is missing")?;
    assert!(gate < upload);
    let mut consumer = spec;
    consumer.sdk_source_bootstrap = false;
    let document = workflow(&consumer)?;
    assert!(document["on"].get("workflow_dispatch").is_some());
    assert!(document["jobs"].get("preview-recovery").is_some());
    assert!(!serde_json::to_string(&document)?.contains("velnor-actions-source"));
    Ok(())
}
