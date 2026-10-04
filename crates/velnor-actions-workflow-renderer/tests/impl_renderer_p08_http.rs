//! Checksummed HTTP Mise tools stay eligible for setup and cache inference.

use std::collections::BTreeMap;
use velnor_actions_contract::{Job, JobTimeout, StepKind};
use velnor_actions_workflow_renderer::cache_p08::{ensure_setup_p08, infer_job_tools};
use velnor_actions_workflow_renderer::shell_step;

use super::impl_renderer_fixtures::*;

#[test]
fn verified_http_tool_spec_gets_setup_and_cache_key() {
    let spec = "http:cargo-machete[url=https://github.com/bnjbvr/cargo-machete/releases/download/v0.9.2/cargo-machete-v0.9.2-x86_64-unknown-linux-musl.tar.gz,checksum=sha256:48200087f54c55aabcd4db4af1e25742b49846c02a1b1bfa134711945b35b2e9]@0.9.2";
    let mut job = Job {
        display_name: "Cargo Machete".to_owned(),
        runs_on: LABEL.to_owned(),
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
                    spec.to_owned(),
                    "--".to_owned(),
                    "cargo".to_owned(),
                    "machete".to_owned(),
                ],
                BTreeMap::new(),
            )
            .expect("machete"),
        ],
    };
    assert_eq!(infer_job_tools(&job), vec![spec.to_owned()]);
    ensure_setup_p08(
        "cargo-machete",
        &mut job,
        &mise(),
        false,
        "x86_64-unknown-linux-gnu",
    )
    .expect("setup cache");
    let Some(StepKind::Action { with, .. }) = job.steps.first().map(|step| &step.kind) else {
        panic!("Setup Mise must bootstrap the verified HTTP tool");
    };
    assert!(with.contains_key("cache_key"), "{with:?}");
    assert!(with.contains_key("cache"), "{with:?}");
}
