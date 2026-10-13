//! P08 tool-spec validation cases.

use std::collections::BTreeMap;
use velnor_actions_contract::{Job, JobTimeout, WorkflowPolicy};
use velnor_actions_workflow_renderer::cache_p08::infer_job_tools;
use velnor_actions_workflow_renderer::{render_workflow_ir_strict, shell_step};

use super::impl_renderer_fixtures::*;

#[test]
fn http_backend_spec_gets_mise_setup_in_generated_workflow() {
    let spec = concat!(
        "http:cargo-machete[url=https://github.com/bnjbvr/cargo-machete/releases/",
        "download/v0.9.2/cargo-machete-v0.9.2-x86_64-unknown-linux-musl.tar.gz,",
        "checksum=sha256:48200087f54c55aabcd4db4af1e25742b49846c02a1b1bfa134711945b35b2e9]@0.9.2"
    )
    .to_owned();
    let job = Job {
        display_name: "Cargo Machete".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            shell_step(
                "Run cargo-machete",
                vec![
                    "mise".to_owned(),
                    "--no-config".to_owned(),
                    "--no-env".to_owned(),
                    "--no-hooks".to_owned(),
                    "exec".to_owned(),
                    spec.clone(),
                    "--".to_owned(),
                    "cargo".to_owned(),
                    "machete".to_owned(),
                ],
                BTreeMap::new(),
            )
            .expect("machete command"),
        ],
    };
    assert_eq!(infer_job_tools(&job), vec![spec.clone()]);

    let yaml = render_workflow_ir_strict(
        &fixture_ir(vec![("cargo-machete".to_owned(), job)]),
        WorkflowPolicy::VelnorRepositoryV1,
        None,
        &fixture_ctx(),
        &mise(),
    )
    .expect("render cargo-machete workflow");
    let names = step_names(&yaml, "cargo-machete");
    let setup = names.iter().position(|name| name == "Setup Mise");
    let command = names.iter().position(|name| name == "Run cargo-machete");
    assert!(
        matches!((setup, command), (Some(setup), Some(command)) if setup < command),
        "generated job must set up Mise before executing the HTTP backend:\n{yaml}"
    );
}
