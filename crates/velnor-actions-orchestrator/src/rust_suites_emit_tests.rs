use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::StepKind;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_rust::{
    CompileDriver as RustCompileDriver, NextestProfile, ProfileSource, RustExecutionProfile,
    TestRunner, WorkspaceRecord,
};
use velnor_actions_workflow_renderer::CompileDriver;
use velnor_actions_workflow_renderer::rust_suites::{build_ci_suite_jobs, render_suites_workflows};

use super::build_render_spec;
use crate::discover::{Discovery, PlannedWorkspace};
use crate::workflow::ToolchainPhase;

fn discovery(driver: RustCompileDriver) -> Discovery {
    Discovery {
        workspaces: vec![PlannedWorkspace {
            record: WorkspaceRecord {
                workspace_root: String::new(),
                members: Vec::new(),
                packages: Vec::new(),
                edges: Vec::new(),
                skipped_edges: Vec::new(),
                has_external_dependencies: false,
            },
            profile: RustExecutionProfile {
                compile_driver: driver,
                test_runner: TestRunner::CargoNextest,
                evidence: Vec::new(),
                driver_source: ProfileSource::Declared,
                runner_source: ProfileSource::Declared,
                nextest_profile: NextestProfile::Default,
                nextest_config: None,
                run_ignored: None,
            },
            recommendations: Vec::new(),
            findings: Vec::new(),
        }],
        ..Discovery::default()
    }
}

fn spec_for(driver: RustCompileDriver) -> Result<super::RustSuitesRenderSpec, Box<dyn Error>> {
    let mut config = crate::pins::tests::config_with(BTreeMap::default());
    config.stacks.rust = Some(serde_json::from_value(serde_json::json!({
        "suites": [{
            "id": "smoke",
            "profile": "default",
            "workflow": "smoke",
            "triggers": ["pull_request", "workflow_dispatch"]
        }]
    }))?);
    build_render_spec(
        &config,
        "ubuntu-26.04",
        &discovery(driver),
        &ToolCatalog::pinned(),
    )?
    .ok_or_else(|| "configured suite spec is missing".into())
}

fn assert_script_phase(step: &velnor_actions_contract::Step, phase: ToolchainPhase, marker: &str) {
    let StepKind::Shell { run, .. } = &step.kind else {
        panic!("suite verifier is a shell step");
    };
    assert_eq!(run.first().map(String::as_str), Some("sh"));
    assert_eq!(run.get(1).map(String::as_str), Some("-c"));
    let script = run.get(2).expect("verifier script");
    let prefix = format!(
        "{} ",
        velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude()
    );
    assert!(
        script.starts_with(&prefix),
        "credential prelude must lead: {script}"
    );
    assert!(
        !script[prefix.len()..].starts_with(&prefix),
        "credential prelude must appear once: {script}"
    );
    assert!(
        script.contains(marker),
        "{phase:?} verifier marker: {script}"
    );
}

fn assert_yaml_phase(workflow: &str, name: &str, marker: &str) {
    let needle = format!("      - name: {name}");
    let start = workflow.find(&needle).expect("rendered verifier step");
    let tail = &workflow[start..];
    let end = tail.find("\n      - name:").unwrap_or(tail.len());
    let step = &tail[..end];
    assert!(
        step.contains(velnor_actions_workflow_renderer::VERIFY_VERIFIER_SHELL),
        "trusted shell: {step}"
    );
    assert!(
        step.contains(velnor_actions_workflow_renderer::VERIFY_PATH),
        "fixed helper path: {step}"
    );
    assert!(step.contains(marker), "{step}");
}

#[test]
fn suite_specs_keep_both_verifier_phases_for_cargo_and_mbx() -> Result<(), Box<dyn Error>> {
    let catalog = ToolCatalog::pinned();
    for driver in [RustCompileDriver::Cargo, RustCompileDriver::Mbx] {
        let spec = spec_for(driver)?;
        assert_eq!(
            spec.compile_driver,
            match driver {
                RustCompileDriver::Cargo => CompileDriver::Cargo,
                RustCompileDriver::Mbx => CompileDriver::Mbx,
            }
        );
        for (phase, actual) in [
            (ToolchainPhase::Restored, &spec.verify_restored_rust_argv),
            (ToolchainPhase::Installed, &spec.verify_rust_argv),
        ] {
            assert_eq!(
                actual,
                &crate::workflow::verify_rust_owned_argv(&catalog, phase)
            );
        }

        let jobs = build_ci_suite_jobs(&spec)?;
        let ci_job = jobs.get("suite-smoke").ok_or("CI suite job is missing")?;
        for (name, phase, marker) in [
            (
                "Verify restored Rust toolchain",
                ToolchainPhase::Restored,
                "no restored Rust",
            ),
            (
                velnor_actions_workflow_renderer::VERIFY_RUST_TOOLCHAIN_STEP,
                ToolchainPhase::Installed,
                "rust_toolchain_bytes_mismatch",
            ),
        ] {
            let step = ci_job
                .steps
                .iter()
                .find(|step| step.name == name)
                .ok_or("CI suite verifier step is missing")?;
            assert_script_phase(step, phase, marker);
        }

        let files = render_suites_workflows(&spec)?;
        let yaml = files
            .iter()
            .find(|file| file.path == ".github/workflows/smoke.yml")
            .ok_or("standalone suite workflow is missing")?;
        assert_yaml_phase(
            &yaml.bytes,
            "Verify restored Rust toolchain",
            "no restored Rust",
        );
        assert_yaml_phase(
            &yaml.bytes,
            velnor_actions_workflow_renderer::VERIFY_RUST_TOOLCHAIN_STEP,
            "rust_toolchain_bytes_mismatch",
        );
    }
    Ok(())
}
