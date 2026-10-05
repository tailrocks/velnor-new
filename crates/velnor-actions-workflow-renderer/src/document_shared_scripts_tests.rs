use std::collections::BTreeMap;

use velnor_actions_contract::workflow::permissions::PermissionLevel;
use velnor_actions_contract::{Job, JobTimeout, Permissions, StepKind, digest_b3};

use super::{SCRIPT_ROOT, compact_oversized_workflow, share_trusted_scripts};
use crate::lane_share::LaneShare;
use crate::render::RenderContext;
use crate::{marker, steps, toolchain_env};

#[cfg(unix)]
#[path = "document_shared_scripts_execution_tests.rs"]
mod execution_tests;

fn context() -> RenderContext {
    RenderContext {
        generator_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: format!("actions/checkout@{}", "0".repeat(40)),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        verification_tasks: Vec::new(),
        plan_consumer_env: BTreeMap::new(),
    }
}

fn fixture_job(ctx: &RenderContext, id: &str, actions_read: bool) -> Job {
    let action_uses = "jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6";
    let input_env = BTreeMap::from([
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
    ]);
    let steps = steps::mbx_steps_for_driver(
        action_uses,
        steps::CompileDriver::Mbx,
        "1.21.1",
        "1.98.1",
        input_env,
    )
    .expect("valid MBX factory input")
    .expect("MBX driver emits a full triplet");
    let checkout = steps::checkout_step(&ctx.checkout_uses).expect("canonical checkout");
    let before = steps::shell_step(
        "Keep before verifier",
        vec!["sh".to_owned(), "-c".to_owned(), "printf before".to_owned()],
        BTreeMap::new(),
    )
    .expect("valid prefix");
    let after = steps::shell_step(
        "Keep after verifier",
        vec!["sh".to_owned(), "-c".to_owned(), "printf after".to_owned()],
        BTreeMap::new(),
    )
    .expect("valid suffix");
    Job {
        display_name: format!("{id} verification"),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: vec!["plan".to_owned(), "policy".to_owned()],
        condition: Some("always()".to_owned()),
        permissions: Some(Permissions {
            contents: PermissionLevel::Read,
            actions: if actions_read {
                PermissionLevel::Read
            } else {
                PermissionLevel::None
            },
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
        }),
        environment: Some(format!("{id}-validation")),
        steps: vec![
            checkout,
            before,
            steps[0].clone(),
            steps[1].clone(),
            steps[2].clone(),
            after,
        ],
    }
}

fn shared_fixture(ctx: &RenderContext) -> LaneShare {
    LaneShare {
        jobs: BTreeMap::from([
            ("linux".to_owned(), fixture_job(ctx, "linux", true)),
            ("macos".to_owned(), fixture_job(ctx, "macos", false)),
        ]),
        calls: BTreeMap::new(),
        checkouts: BTreeMap::new(),
        env_steps: BTreeMap::new(),
        prefixes: BTreeMap::new(),
        preludes: BTreeMap::new(),
        postludes: BTreeMap::new(),
        files: Vec::new(),
    }
}

#[test]
fn repeated_exact_factory_bodies_preserve_all_step_and_job_metadata() {
    let ctx = context();
    let mut shared = shared_fixture(&ctx);
    let original = shared.jobs.clone();
    let files = share_trusted_scripts(&mut shared, &ctx).expect("share factory bodies");

    assert_eq!(files.len(), 2);
    assert!(files.iter().all(|file| file.path.starts_with(SCRIPT_ROOT)));
    assert!(
        files
            .iter()
            .all(|file| { marker::check_first_line(&file.bytes, &ctx.generator_version).is_ok() })
    );
    assert_content_addressed_paths(&files);
    assert_expands_exactly(&original, &shared.jobs, &files, &ctx.generator_version);

    let mut repeat = shared_fixture(&ctx);
    let repeated = share_trusted_scripts(&mut repeat, &ctx).expect("deterministic share");
    assert_eq!(files, repeated);
    assert_expands_exactly(&original, &repeat.jobs, &repeated, &ctx.generator_version);
}

#[test]
fn oversized_workflow_renders_once_after_trusted_scripts_are_shared() {
    let ctx = context();
    let mut shared = shared_fixture(&ctx);
    let render_calls = std::cell::Cell::new(0);
    let compacted = compact_oversized_workflow(
        &mut shared,
        &ctx,
        "x".repeat(crate::workflow_size::MAX_WORKFLOW_BYTES + 1),
        |shared| {
            render_calls.set(render_calls.get() + 1);
            assert_eq!(shared.files.len(), 2);
            Ok("compacted".to_owned())
        },
    )
    .expect("compact eligible factory bodies");

    assert_eq!(compacted, "compacted");
    assert_eq!(render_calls.get(), 1);
}

#[test]
fn under_limit_workflow_skips_sharing_and_rerender() {
    let ctx = context();
    let mut shared = shared_fixture(&ctx);
    let original = shared.jobs.clone();
    let render_calls = std::cell::Cell::new(0);
    let text = compact_oversized_workflow(&mut shared, &ctx, "small".to_owned(), |_| {
        render_calls.set(render_calls.get() + 1);
        Ok("unexpected".to_owned())
    })
    .expect("leave under-limit workflow alone");

    assert_eq!(text, "small");
    assert_eq!(render_calls.get(), 0);
    assert_jobs_equal(&original, &shared.jobs);
    assert!(shared.files.is_empty());
}

#[test]
fn one_job_or_modified_triplet_does_not_emit_shared_files() {
    let ctx = context();
    let mut one = shared_fixture(&ctx);
    let removed = one.jobs.remove("macos");
    assert!(removed.is_some());
    let before = one.jobs.clone();
    assert!(
        share_trusted_scripts(&mut one, &ctx)
            .expect("single eligible job")
            .is_empty()
    );
    assert_jobs_equal(&before, &one.jobs);

    let mut altered = shared_fixture(&ctx);
    for job in altered.jobs.values_mut() {
        let step = job
            .steps
            .iter_mut()
            .find(|step| step.name == steps::MBX_PREFLIGHT_NAME)
            .expect("preflight step");
        let StepKind::Shell { run, .. } = &mut step.kind else {
            panic!("preflight factory output is shell")
        };
        run[2].push_str("; printf altered");
    }
    let before = altered.jobs.clone();
    assert!(
        share_trusted_scripts(&mut altered, &ctx)
            .expect("modified triplets remain inline")
            .is_empty()
    );
    assert_jobs_equal(&before, &altered.jobs);
}

#[test]
fn noncontiguous_factory_steps_are_not_shared() {
    let ctx = context();
    let mut shared = shared_fixture(&ctx);
    for job in shared.jobs.values_mut() {
        let insertion = job
            .steps
            .iter()
            .position(|step| step.name == steps::MBX_RESTORE_NAME)
            .expect("MBX action step");
        let separator = steps::shell_step(
            "Intervening work",
            vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
            BTreeMap::new(),
        )
        .expect("valid separator");
        job.steps.insert(insertion, separator);
    }
    let before = shared.jobs.clone();
    assert!(
        share_trusted_scripts(&mut shared, &ctx)
            .expect("noncontiguous source steps remain inline")
            .is_empty()
    );
    assert_jobs_equal(&before, &shared.jobs);
}

#[test]
fn custom_or_repeated_checkout_is_not_eligible() {
    let ctx = context();
    let mut custom = shared_fixture(&ctx);
    for job in custom.jobs.values_mut() {
        let StepKind::Action { with, .. } = &mut job.steps[0].kind else {
            panic!("checkout step is action")
        };
        with.insert("path".to_owned(), "elsewhere".to_owned());
    }
    assert!(
        share_trusted_scripts(&mut custom, &ctx)
            .expect("custom checkout stays inline")
            .is_empty()
    );

    let mut repeated = shared_fixture(&ctx);
    let extra = steps::checkout_step(&ctx.checkout_uses).expect("checkout");
    repeated
        .jobs
        .get_mut("linux")
        .expect("Linux job")
        .steps
        .push(extra);
    assert!(
        share_trusted_scripts(&mut repeated, &ctx)
            .expect("duplicate checkout stays inline")
            .is_empty()
    );
}

#[test]
fn exact_markers_and_paths_follow_the_factory_body_digest() {
    let ctx = context();
    let mut shared = shared_fixture(&ctx);
    let files = share_trusted_scripts(&mut shared, &ctx).expect("share bodies");
    for file in files {
        let (_, body) = file.bytes.split_once('\n').expect("marked body");
        let script_name = file.path.rsplit('/').next().expect("script filename");
        let expected = digest_b3(format!("sh\0{body}").as_bytes());
        assert_eq!(script_name, format!("sh-{expected}.sh"));
        assert!(file.path.starts_with(".github/scripts/velnor-shared/"));
    }
}

fn assert_content_addressed_paths(files: &[crate::tree::RenderedFile]) {
    assert!(files.iter().all(|file| {
        let Some(name) = file.path.strip_prefix(&format!("{SCRIPT_ROOT}/")) else {
            return false;
        };
        let Some((stem, extension)) = name.rsplit_once('.') else {
            return false;
        };
        let Some((dialect, digest)) = stem.split_once("-b3-") else {
            return false;
        };
        matches!((dialect, extension), ("sh", "sh") | ("bash", "bash"))
            && digest.len() == 64
            && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    }));
}

fn assert_expands_exactly(
    original: &BTreeMap<String, Job>,
    compacted: &BTreeMap<String, Job>,
    files: &[crate::tree::RenderedFile],
    version: &str,
) {
    assert_eq!(
        original.keys().collect::<Vec<_>>(),
        compacted.keys().collect::<Vec<_>>()
    );
    for (job_id, before) in original {
        let after = compacted.get(job_id).expect("same job id");
        let mut expanded = after.clone();
        assert_eq!(before.steps.len(), after.steps.len());
        for (index, original_step) in before.steps.iter().enumerate() {
            let actual = &mut expanded.steps[index];
            if let StepKind::Shell { run, .. } = &mut actual.kind
                && run.len() == 3
                && let Some(file) = files.iter().find(|file| {
                    run[2]
                        == toolchain_env::with_credential_unset_script(&format!(
                            ". './{}'",
                            file.path
                        ))
                })
            {
                assert!(marker::check_first_line(&file.bytes, version).is_ok());
                let (_, body) = file.bytes.split_once('\n').expect("marked body");
                run[2] = toolchain_env::with_credential_unset_script(body);
            }
            assert_eq!(actual, original_step, "{job_id} step {index}");
        }
        assert_job_metadata_equal(before, &expanded, job_id);
    }
}

fn assert_jobs_equal(left: &BTreeMap<String, Job>, right: &BTreeMap<String, Job>) {
    assert_eq!(
        left.keys().collect::<Vec<_>>(),
        right.keys().collect::<Vec<_>>()
    );
    for (job_id, original) in left {
        let actual = right.get(job_id).expect("same job id");
        assert_job_metadata_equal(original, actual, job_id);
        assert_eq!(original.steps, actual.steps, "steps changed for {job_id}");
    }
}

fn assert_job_metadata_equal(left: &Job, right: &Job, job_id: &str) {
    assert_eq!(
        left.display_name, right.display_name,
        "display name {job_id}"
    );
    assert_eq!(left.runs_on, right.runs_on, "runner {job_id}");
    assert_eq!(
        left.timeout_minutes, right.timeout_minutes,
        "timeout {job_id}"
    );
    assert_eq!(left.needs, right.needs, "needs {job_id}");
    assert_eq!(left.condition, right.condition, "condition {job_id}");
    assert_eq!(left.permissions, right.permissions, "permissions {job_id}");
    assert_eq!(left.environment, right.environment, "environment {job_id}");
}
