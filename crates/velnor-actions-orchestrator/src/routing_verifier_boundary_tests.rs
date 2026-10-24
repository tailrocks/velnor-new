use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_contract::{ExecutionConfig, RoutingWorkflow, VelnorConfig};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::{
    VERIFY_PATH, VERIFY_VERIFIER_SHELL, render_schema2_workflows,
};

use super::workflow_request;

fn verifier_steps(workflow: &str) -> Vec<&str> {
    let needle = "name: Verify Rust channel manifest";
    let mut rest = workflow;
    let mut steps = Vec::new();
    while let Some(start) = rest.find(needle) {
        let tail = &rest[start..];
        let end = tail.find("\n      - name:").unwrap_or(tail.len());
        steps.push(&tail[..end]);
        rest = &tail[needle.len()..];
    }
    steps
}

fn job<'a>(workflow: &'a str, id: &str) -> &'a str {
    let marker = format!("  {id}:");
    let start = workflow.find(&marker).expect("qualification job");
    let rest = &workflow[start..];
    let body_len = rest[marker.len()..]
        .split_inclusive('\n')
        .take_while(|line| !line.starts_with("  ") || line.starts_with("    "))
        .map(str::len)
        .sum::<usize>();
    &rest[..marker.len() + body_len]
}

fn assert_verifier_boundary(step: &str) {
    assert!(
        step.contains(&format!("shell: {VERIFY_VERIFIER_SHELL}")),
        "{step}"
    );
    assert!(step.contains(&format!("PATH: {VERIFY_PATH}")), "{step}");
    assert!(step.contains("function_env_scan_complete"), "{step}");
    assert!(step.contains("rust_toolchain_bytes_mismatch"), "{step}");
    assert!(
        step.contains("${RUSTUP_HOME:-${MISE_RUSTUP_HOME:-$HOME/.rustup}}"),
        "the verifier retains its canonical default Rust home fallback"
    );
    assert!(
        step.lines().all(|line| {
            let line = line.trim_start();
            !line.starts_with("RUSTUP_HOME:") && !line.starts_with("CARGO_HOME:")
        }),
        "the verifier step must not override the qualification job's Rust and Cargo homes"
    );
}

fn assert_qualification_job_homes(job: &str, cache_mode: &str) {
    let home = "${{ github.workspace }}/.velnor-mbx-cache-qualification";
    for (key, suffix) in [
        ("CARGO_HOME", "cargo"),
        ("MISE_CARGO_HOME", "cargo"),
        ("MISE_RUSTUP_HOME", "rustup"),
        ("RUSTUP_HOME", "rustup"),
    ] {
        let field = format!("      {key}: {home}/{suffix}\n");
        assert!(
            job.contains(&field),
            "{key} must retain its isolated job home\n{job}"
        );
    }
    assert!(
        job.contains(&format!("      ACTIONS_CACHE_MODE: {cache_mode}\n")),
        "writer/reader cache mode remains {cache_mode}\n{job}"
    );
}

#[test]
fn qualification_routes_the_same_owned_verifier_to_writer_and_reader() -> Result<(), Box<dyn Error>>
{
    let mut config = crate::pins::tests::config_with(BTreeMap::default());
    config.schema = VelnorConfig::SCHEMA_V2;
    let mut execution = ExecutionConfig::hosted_default("ubuntu-26.04")?;
    execution.workflows = BTreeSet::from([RoutingWorkflow::Qualification]);
    config.execution = Some(execution.clone());
    let catalog = ToolCatalog::pinned();
    let expected = crate::workflow::verify_rust_argv(&catalog);
    let prelude = format!(
        "{} ",
        velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude()
    );
    assert!(expected[2].starts_with(&prelude));

    let routing = super::workflow_routing_policy(&config, None)?;
    let request = workflow_request(&config, &execution, &catalog, "0.1.44", &routing)?;
    let pins = request
        .mbx_qualification
        .as_ref()
        .expect("qualification has MBX verifier input");
    assert_eq!(pins.verify_rust_argv, expected);
    assert!(pins.verify_rust_argv[2].contains("rust_toolchain_bytes_mismatch"));

    let files = render_schema2_workflows(&request)?;
    let rendered = files
        .iter()
        .find(|file| file.path == ".github/workflows/qualification.yml")
        .ok_or("qualification workflow is missing")?;
    for (id, cache_mode) in [
        ("mbx-cache-write-hosted", "write"),
        ("mbx-cache-read-hosted", "read"),
    ] {
        let qualification_job = job(&rendered.bytes, id);
        assert_qualification_job_homes(qualification_job, cache_mode);
        let steps = verifier_steps(qualification_job);
        assert_eq!(steps.len(), 1, "{id} has one Rust verifier");
        assert_verifier_boundary(steps[0]);
    }
    let paired_builder_steps = verifier_steps(job(&rendered.bytes, "paired-sdk-build"));
    assert_eq!(
        paired_builder_steps.len(),
        1,
        "the paired fixture builder has one Rust verifier"
    );
    assert_verifier_boundary(paired_builder_steps[0]);
    assert_eq!(
        rendered
            .bytes
            .matches("name: Verify Rust channel manifest")
            .count(),
        3,
        "the two cache lanes and paired fixture builder each use one verifier"
    );
    Ok(())
}
