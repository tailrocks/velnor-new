use std::collections::BTreeMap;

use velnor_actions_contract_workflow::{JobOutput, Step, StepId, StepKind, StepRole};
use velnor_actions_workflow_cache::cache_steps::{
    CompileDriver, cache_action_step, mbx_steps_for_driver,
};
use velnor_actions_workflow_jobs::{PreseedStageSource, download_plan_step};
use velnor_actions_workflow_steps::steps::shell_step;
use velnor_actions_workflow_tree::rendered::RenderedFile;

use crate::document_lanes::factor_static_task_prefixes;
use crate::lane_share::tests::{lane_job, scale_token};
use crate::lane_share::{LaneShare, SharedActionCall};

const ACTION_SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const MBX_USES: &str = "jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6";

fn bootstrap_steps(tool: &str) -> Vec<Step> {
    let context = super::ctx();
    let target = "x86_64-unknown-linux-gnu";
    let mut steps = vec![
        velnor_actions_workflow_jobs::preseed_download_step()
            .expect("preseed download step is valid"),
        velnor_actions_workflow_jobs::preseed_manifest_verify_step(target)
            .expect("preseed manifest verification step is valid"),
        velnor_actions_workflow_jobs::preseed_stage_step(
            PreseedStageSource::DownloadedArtifact,
            &context.staged_binary,
        )
        .expect("preseed stage step is valid"),
        download_plan_step().expect("plan download step is valid"),
        shell_role(
            "Prepare pinned tools",
            vec!["mise".to_owned(), "install".to_owned(), tool.to_owned()],
            BTreeMap::new(),
            StepRole::PreparePinnedTools,
        ),
        shell_role(
            "Prepare Rust components",
            vec![
                "rustup".to_owned(),
                "component".to_owned(),
                "add".to_owned(),
            ],
            BTreeMap::new(),
            StepRole::PrepareRustComponents,
        ),
        source_restore_step(),
    ];
    let homes = rust_homes();
    let mbx = mbx_steps_for_driver(MBX_USES, CompileDriver::Mbx, "1.21.1", "1.98.1", homes)
        .expect("MBX steps are valid")
        .expect("MBX driver has configured steps");
    steps.extend(mbx);
    steps
}

fn shell_role(name: &str, run: Vec<String>, env: BTreeMap<String, String>, role: StepRole) -> Step {
    let mut step = shell_step(name, run, env).expect("shell step is valid");
    step.role = Some(role);
    step
}

fn rust_homes() -> BTreeMap<String, String> {
    let cargo = "${{ runner.temp }}/velnor/cargo".to_owned();
    let rustup = "${{ runner.temp }}/velnor/rustup".to_owned();
    BTreeMap::from([
        ("CARGO_HOME".to_owned(), cargo.clone()),
        ("MISE_CARGO_HOME".to_owned(), cargo),
        ("MISE_RUSTUP_HOME".to_owned(), rustup.clone()),
        ("RUSTUP_HOME".to_owned(), rustup),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
    ])
}

fn source_restore_step() -> Step {
    let paths = vec!["$CARGO_HOME/registry/index".to_owned()];
    let mut step = cache_action_step(
        true,
        &format!("actions/cache/restore@{ACTION_SHA}"),
        "sources",
        "velnor-sources-key",
        &["velnor-sources-restore-".to_owned()],
        &paths,
    )
    .expect("Cargo source restore action is valid");
    step.name = "Restore Cargo sources".to_owned();
    step.role = Some(StepRole::CargoSourcesRestore);
    step
}

fn report_postlude() -> Vec<Step> {
    vec![
        Step {
            condition: Some("always()".to_owned()),
            ..super::echo_step(90, "upload report")
        },
        Step {
            condition: Some("success() && github.event_name == 'push'".to_owned()),
            role: Some(StepRole::ToolsCacheSave),
            ..super::echo_step(91, "save cache")
        },
    ]
}

fn shared_prefixes(variants: usize) -> LaneShare {
    let mut jobs = BTreeMap::new();
    let mut calls = BTreeMap::new();
    let mut checkouts = BTreeMap::new();
    let mut env_steps = BTreeMap::new();
    let mut prefixes = BTreeMap::new();
    let mut preludes = BTreeMap::new();
    let mut postludes = BTreeMap::new();
    let scale = scale_token();
    for index in 0..variants {
        let steps = bootstrap_steps(&format!(
            "rust@1.98.1 aqua:nextest-rs/nextest/cargo-nextest@0.9.{index}"
        ));
        for (suffix, runs_on) in [
            ("__hosted", super::HOSTED_RUNS),
            ("__local", scale.as_str()),
        ] {
            let id = format!("rust-demo-{index}{suffix}");
            let mut job = lane_job("Rust task", runs_on, Vec::new());
            job.outputs = vec![
                JobOutput::task_report_artifact_id(),
                JobOutput::task_report_check_run_id(),
            ];
            jobs.insert(id.clone(), job);
            calls.insert(
                id.clone(),
                SharedActionCall {
                    prelude_uses: Vec::new(),
                    uses: format!("./.github/actions/task-rust-demo-{index}"),
                    inputs: Vec::new(),
                },
            );
            checkouts.insert(id.clone(), super::super::checkout());
            env_steps.insert(id.clone(), Vec::new());
            prefixes.insert(id.clone(), Vec::new());
            preludes.insert(id.clone(), steps.clone());
            postludes.insert(id, report_postlude());
        }
    }
    LaneShare {
        jobs,
        calls,
        checkouts,
        env_steps,
        prefixes,
        preludes,
        postludes,
        files: Vec::new(),
    }
}

#[test]
fn exact_provider_pairs_share_three_static_prefix_variants_only() {
    let mut shared = shared_prefixes(3);
    shared.files.push(RenderedFile {
        path: ".github/actions/rust-0/action.yml".to_owned(),
        bytes: String::new(),
    });
    let original_outputs = shared
        .jobs
        .iter()
        .map(|(id, job)| (id.clone(), job.outputs.clone()))
        .collect::<BTreeMap<_, _>>();
    let original_postludes = shared.postludes.clone();

    factor_static_task_prefixes(&mut shared, &super::ctx()).expect("typed prefix factoring");

    assert_eq!(shared.files.len(), 4);
    assert_eq!(
        shared.jobs.keys().collect::<Vec<_>>(),
        original_outputs.keys().collect::<Vec<_>>()
    );
    for (id, job) in &shared.jobs {
        assert_eq!(
            job.outputs, original_outputs[id],
            "{id} output owners changed"
        );
    }
    assert_eq!(shared.postludes, original_postludes);
    assert_eq!(shared.preludes.len(), 6);
    for (id, steps) in &shared.preludes {
        assert!(steps.is_empty(), "factored prelude {id} was retained");
        let uses = &shared.calls[id].prelude_uses;
        assert_eq!(uses.len(), 1, "{id}");
        assert!(uses[0].starts_with("./.github/actions/rust-"));
        assert_ne!(uses[0], "./.github/actions/rust-0");
    }
    assert!(shared.files.iter().any(|file| {
        file.bytes.contains("Verify native MBX version")
            && file.bytes.contains("Prepare pinned tools")
    }));
    let yaml = super::render_jobs(&super::workflow_ir(), &shared, &super::ctx())
        .expect("canonical local calls render");
    for call in shared.calls.values() {
        let uses = &call.prelude_uses[0];
        assert!(yaml.contains(&format!("uses: {uses} # zizmor: ignore[self-repository]")));
    }
}

#[test]
fn output_owners_and_non_success_postludes_stay_outer() {
    let mut shared = shared_prefixes(1);
    let expected_outputs = shared
        .jobs
        .iter()
        .map(|(id, job)| (id.clone(), job.outputs.clone()))
        .collect::<BTreeMap<_, _>>();
    let expected_postludes = shared.postludes.clone();
    let ids = shared.preludes.keys().cloned().collect::<Vec<_>>();
    shared
        .preludes
        .get_mut(&ids[0])
        .expect("first task prelude exists")[0]
        .id = Some(StepId::Plan);

    factor_static_task_prefixes(&mut shared, &super::ctx()).expect("unmatched prefix stays outer");

    assert_eq!(shared.files, Vec::<RenderedFile>::new());
    for (id, job) in &shared.jobs {
        assert_eq!(
            job.outputs, expected_outputs[id],
            "{id} output owners changed"
        );
    }
    assert_eq!(shared.postludes, expected_postludes);
    assert_eq!(shared.preludes[&ids[0]][0].id, Some(StepId::Plan));
    assert_eq!(shared.preludes[&ids[1]].len(), 10);
}

#[test]
fn indexed_workflow_context_inside_prefix_fails_closed() {
    let mut shared = shared_prefixes(1);
    let ids = shared.preludes.keys().cloned().collect::<Vec<_>>();
    let StepKind::Shell { run, .. } = &mut shared
        .preludes
        .get_mut(&ids[0])
        .expect("first task prelude exists")[4]
        .kind
    else {
        panic!("Prepare pinned tools must be a shell step");
    };
    run.push("${{ needs['plan'].outputs.covered_tasks }}".to_owned());

    factor_static_task_prefixes(&mut shared, &super::ctx()).expect("unsupported scope stays outer");

    assert_eq!(shared.files, Vec::<RenderedFile>::new());
    assert_eq!(shared.preludes[&ids[0]].len(), 10);
    assert_eq!(shared.preludes[&ids[1]].len(), 10);
}

#[test]
fn nonterminal_prelude_steps_remain_in_their_original_order() {
    let mut shared = shared_prefixes(1);
    let original = shared.preludes.clone();
    for steps in shared.preludes.values_mut() {
        steps.insert(0, super::echo_step(80, "outer-before"));
        steps.push(super::echo_step(81, "outer-after"));
    }
    let expected = shared.preludes.clone();
    for (id, steps) in &expected {
        assert_eq!(&steps[1..11], &original[id]);
    }

    factor_static_task_prefixes(&mut shared, &super::ctx()).expect("unsupported shape stays outer");

    assert_eq!(shared.files, Vec::<RenderedFile>::new());
    assert!(
        shared
            .calls
            .values()
            .all(|call| call.prelude_uses.is_empty())
    );
    assert_eq!(shared.preludes, expected);
}
