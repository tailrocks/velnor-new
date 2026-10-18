use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_contract::StepKind;

use crate::{RenderContext, setup::MiseSetup, yaml::Yaml};

use super::{ToolsCacheInputs, ToolsCachePayload};

fn pinned_mise() -> MiseSetup {
    MiseSetup {
        uses: "jdx/mise-action@9b200bcecbfd6303e69a4a261b2e09e3757440a7".to_owned(),
        version: "2026.10.4".to_owned(),
        sha256: "a".repeat(64),
    }
}

fn inputs<'a>(
    runs_on: &'a str,
    mise_setup: &'a MiseSetup,
    specs: &'a [String],
    rustup_toolchain: Option<&'a str>,
    components: &'a [String],
) -> ToolsCacheInputs<'a> {
    inputs_with_source(
        runs_on,
        mise_setup,
        specs,
        rustup_toolchain,
        components,
        None,
    )
}

fn inputs_with_source<'a>(
    runs_on: &'a str,
    mise_setup: &'a MiseSetup,
    specs: &'a [String],
    rustup_toolchain: Option<&'a str>,
    components: &'a [String],
    scale_set_image_source_sha: Option<&'a str>,
) -> ToolsCacheInputs<'a> {
    ToolsCacheInputs {
        runs_on,
        target: "x86_64-unknown-linux-gnu",
        mise_setup,
        tool_specs: specs,
        rustup_toolchain,
        rustup_components: components,
        scale_set_image_source_sha,
    }
}

fn payload(inputs: ToolsCacheInputs<'_>) -> ToolsCachePayload {
    ToolsCachePayload::new(inputs).expect("typed tools payload")
}

fn sample_payload() -> ToolsCachePayload {
    let setup = pinned_mise();
    let specs = vec!["rust@1.98.1".to_owned(), "shellcheck@0.11.0".to_owned()];
    let components = vec!["rustfmt".to_owned(), "clippy".to_owned()];
    payload(inputs(
        "ubuntu-26.04",
        &setup,
        &specs,
        Some("1.98.1"),
        &components,
    ))
}

fn action_inputs(
    step: &velnor_actions_contract::Step,
) -> &std::collections::BTreeMap<String, String> {
    match &step.kind {
        StepKind::Action { with, .. } => with,
        kind => panic!("expected action, got {kind:?}"),
    }
}

fn rendered_condition(step: &Step) -> String {
    let context = RenderContext {
        generator_version: "0.1.0".to_owned(),
        report_helper_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        scale_set_selector: None,
        scale_set_image_source_sha: None,
        verified_cache_helpers: std::collections::BTreeMap::new(),
        require_verified_cache_helper: false,
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: format!("actions/checkout@{:040x}", 0),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        mise_quiet: true,
        plan_consumer_env: BTreeMap::new(),
    };
    let job_env = BTreeMap::new();
    let step_context = crate::document_lanes::JobStepContext {
        job_env: &job_env,
        runs_on: Some("ubuntu-26.04"),
        actions_read: false,
    };
    let Yaml::Map(entries) =
        crate::document_steps::step_to_yaml("rust-amq", step, &context, &[], false, &step_context)
            .expect("render consumer step")
    else {
        panic!("a rendered step is a mapping");
    };
    entries
        .into_iter()
        .find_map(|(key, value)| match (key.as_str(), value) {
            ("if", Yaml::Str(condition)) => Some(condition),
            _ => None,
        })
        .expect("rendered save condition")
}

#[path = "cache_p08_tool_payload_identity_tests.rs"]
mod identity_tests;
#[path = "cache_p08_tool_payload_step_tests.rs"]
mod step_tests;
