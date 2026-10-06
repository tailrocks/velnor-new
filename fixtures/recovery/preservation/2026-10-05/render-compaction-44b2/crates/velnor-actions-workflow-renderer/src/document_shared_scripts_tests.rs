use std::collections::BTreeMap;

use velnor_actions_contract::workflow::permissions::PermissionLevel;
use velnor_actions_contract::{Job, JobTimeout, Permissions, StepKind};

use super::{SCRIPT_ROOT, share_trusted_scripts, trusted_shell};
use crate::lane_share::LaneShare;
use crate::render::RenderContext;
use crate::{marker, steps};

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

fn mbx_env(rust_home: &str, cargo_home: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("MISE_RUSTUP_HOME".to_owned(), rust_home.to_owned()),
        ("MISE_CARGO_HOME".to_owned(), cargo_home.to_owned()),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
    ])
}

fn fixture_job(ctx: &RenderContext, id: &str, scale_set: bool) -> Job {
    let action_uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
    let homes = mbx_env(
        &format!("${{ runner.temp }}/velnor/{id}-rustup"),
        &format!("${{ runner.temp }}/velnor/{id}-cargo"),
    );
    let [preflight, restore] = steps::mbx_steps_for_driver(
        &action_uses,
        steps::CompileDriver::Mbx,
        "1.21.1",
        "1.98.1",
        homes,
    )
    .expect("canonical MBX steps")
    .expect("MBX driver emits setup");
    let checkout = steps::checkout_step(&ctx.checkout_uses).expect("root checkout");
    let prefix = steps::shell_step(
        "Keep before setup",
        vec!["sh".to_owned(), "-c".to_owned(), "printf before".to_owned()],
        BTreeMap::new(),
    )
    .expect("prefix step");
    let suffix = steps::shell_step(
        "Keep after setup",
        vec!["sh".to_owned(), "-c".to_owned(), "printf after".to_owned()],
        BTreeMap::new(),
    )
    .expect("suffix step");
    Job {
        display_name: format!("{id} verification"),
        runs_on: if scale_set {
            "scale-set:velnor+ubuntu-26.04-scale-set".to_owned()
        } else {
            "ubuntu-26.04".to_owned()
        },
        timeout_minutes: JobTimeout::CRATE,
        needs: vec!["plan".to_owned(), "policy".to_owned()],
        condition: Some("always()".to_owned()),
        permissions: Some(Permissions {
            contents: PermissionLevel::Read,
            actions: PermissionLevel::Read,
            pull_requests: PermissionLevel::None,
            id_token: PermissionLevel::None,
        }),
        environment: Some(format!("{id}-validation")),
        steps: vec![checkout, prefix, preflight, restore, suffix],
    }
}

fn shared_fixture(ctx: &RenderContext) -> LaneShare {
    let mut jobs = BTreeMap::from([
        ("hosted".to_owned(), fixture_job(ctx, "hosted", false)),
        ("scale".to_owned(), fixture_job(ctx, "scale", true)),
    ]);
    crate::mbx_bundle::append_single_bundle_saves(&mut jobs)
        .expect("canonical cache key and bundle steps");
    LaneShare {
        jobs,
        calls: BTreeMap::new(),
        checkouts: BTreeMap::new(),
        env_steps: BTreeMap::new(),
        prefixes: BTreeMap::new(),
        preludes: BTreeMap::new(),
        postludes: BTreeMap::new(),
        files: Vec::new(),
    }
}

fn expand_shared_steps(
    original: &BTreeMap<String, Job>,
    compacted: &BTreeMap<String, Job>,
    files: &[crate::tree::RenderedFile],
) {
    assert_eq!(original.len(), compacted.len());
    for (job_id, before) in original {
        let after = compacted.get(job_id).expect("same job id");
        let mut expanded = after.clone();
        assert_job_header_eq(before, after, job_id);
        assert_eq!(before.steps.len(), after.steps.len());
        for (index, original_step) in before.steps.iter().enumerate() {
            let actual = &mut expanded.steps[index];
            if let StepKind::Shell { run, .. } = &mut actual.kind
                && let Some(command) = run.get_mut(2)
                && let Some(file) = files.iter().find(|file| {
                    *command
                        == crate::toolchain_env::with_credential_unset_script(&format!(
                            ". './{}'",
                            file.path
                        ))
                })
            {
                let (_, body) = file.bytes.split_once('\n').expect("marked body");
                *command = crate::toolchain_env::with_credential_unset_script(body);
            }
            assert_eq!(actual, original_step, "{job_id} step {index}");
        }
        assert_job_header_eq(before, &expanded, job_id);
    }
}

fn assert_job_header_eq(expected: &Job, actual: &Job, job_id: &str) {
    assert_eq!(expected.display_name, actual.display_name, "{job_id}");
    assert_eq!(expected.runs_on, actual.runs_on, "{job_id}");
    assert_eq!(expected.timeout_minutes, actual.timeout_minutes, "{job_id}");
    assert_eq!(expected.needs, actual.needs, "{job_id}");
    assert_eq!(expected.condition, actual.condition, "{job_id}");
    assert_eq!(expected.permissions, actual.permissions, "{job_id}");
    assert_eq!(expected.environment, actual.environment, "{job_id}");
}

fn assert_job_maps_eq(expected: &BTreeMap<String, Job>, actual: &BTreeMap<String, Job>) {
    assert_eq!(
        expected.keys().collect::<Vec<_>>(),
        actual.keys().collect::<Vec<_>>()
    );
    for (job_id, expected_job) in expected {
        let actual_job = actual.get(job_id).expect("same job id");
        assert_job_header_eq(expected_job, actual_job, job_id);
        assert_eq!(expected_job.steps, actual_job.steps, "{job_id}");
    }
}

#[test]
fn repeated_factory_bodies_keep_order_and_expand_to_exact_original_steps() {
    let ctx = context();
    let mut shared = shared_fixture(&ctx);
    for (job_id, job) in &shared.jobs {
        let (index, step) = job
            .steps
            .iter()
            .enumerate()
            .find(|(_, step)| step.name == steps::MBX_PREFLIGHT_NAME)
            .expect("preflight factory step");
        let StepKind::Shell { env, .. } = &step.kind else {
            panic!("preflight is a shell step")
        };
        let StepKind::Action {
            uses,
            with,
            env: action_env,
        } = &job.steps[index + 2].kind
        else {
            panic!("MBX restore is an action")
        };
        let [expected, _] = steps::mbx_steps_for_driver(
            uses,
            steps::CompileDriver::Mbx,
            action_env.get("VELNOR_MBX_VERSION").expect("MBX version"),
            with.get("toolchain").expect("Rust toolchain"),
            super::factory_input_env(env),
        )
        .expect("regenerate preflight")
        .expect("MBX preflight");
        assert_eq!(step, &expected, "factory output changed for {job_id}");
        assert!(
            super::canonical_preflight(job, index, step),
            "preflight source factory did not match for {job_id}: {step:#?}"
        );
    }
    let original = shared.jobs.clone();
    let files = share_trusted_scripts(&mut shared, &ctx).expect("share exact factory bodies");

    assert_eq!(files.len(), 2, "generated paths: {files:?}");
    assert!(files.iter().all(|file| file.path.starts_with(SCRIPT_ROOT)));
    assert!(
        files
            .iter()
            .all(|file| marker::check_first_line(&file.bytes, "0.1.0").is_ok())
    );
    assert_content_addressed_paths(&files);
    expand_shared_steps(&original, &shared.jobs, &files);

    let mut repeat = shared_fixture(&ctx);
    let repeated_files = share_trusted_scripts(&mut repeat, &ctx).expect("deterministic share");
    assert_eq!(files, repeated_files);
    assert_job_maps_eq(&shared.jobs, &repeat.jobs);
}

fn assert_content_addressed_paths(files: &[crate::tree::RenderedFile]) {
    assert_eq!(files.len(), 2);
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

#[test]
fn one_job_and_non_factory_shells_do_not_create_unneeded_files() {
    let ctx = context();
    let mut one_job = shared_fixture(&ctx);
    one_job.jobs.remove("scale");
    let before = one_job.jobs.clone();
    assert!(
        share_trusted_scripts(&mut one_job, &ctx)
            .expect("single eligible job")
            .is_empty()
    );
    assert_job_maps_eq(&before, &one_job.jobs);

    let mut changed = shared_fixture(&ctx);
    for job in changed.jobs.values_mut() {
        let step = job
            .steps
            .iter_mut()
            .find(|step| step.name == steps::MBX_PREFLIGHT_NAME)
            .expect("preflight factory step");
        let StepKind::Shell { run, .. } = &mut step.kind else {
            panic!("preflight remains a shell step")
        };
        run[2].push_str("; printf altered");
    }
    let files = share_trusted_scripts(&mut changed, &ctx).expect("reject altered bodies");
    assert_eq!(files.len(), 1);
    assert!(
        files[0]
            .path
            .starts_with(&format!("{SCRIPT_ROOT}/bash-b3-"))
    );
}

#[test]
fn custom_checkout_identity_and_later_checkout_fail_closed() {
    let ctx = context();
    for option in ["repository", "ref", "path", "sparse-checkout"] {
        let mut shared = shared_fixture(&ctx);
        for job in shared.jobs.values_mut() {
            let StepKind::Action { with, .. } = &mut job.steps[0].kind else {
                panic!("root checkout is an action")
            };
            with.insert(option.to_owned(), "other-repository-state".to_owned());
        }
        assert!(
            share_trusted_scripts(&mut shared, &ctx)
                .expect("custom checkout is ineligible")
                .is_empty()
        );
    }

    let mut later_checkout = shared_fixture(&ctx);
    let checkout = steps::checkout_step(&ctx.checkout_uses).expect("checkout");
    later_checkout
        .jobs
        .get_mut("hosted")
        .expect("hosted job")
        .steps
        .push(checkout);
    assert!(
        share_trusted_scripts(&mut later_checkout, &ctx)
            .expect("duplicate checkout is ineligible")
            .is_empty()
    );
}

#[test]
fn source_sensitive_shell_constructs_are_rejected() {
    let base = fixture_job(&context(), "one", false)
        .steps
        .into_iter()
        .find(|step| step.name == steps::MBX_PREFLIGHT_NAME)
        .expect("preflight");
    for body in [
        "printf '%s' \"$BASH_SOURCE\"",
        "printf '%s' \"$0\"",
        "printf '%s' \"$LINENO\"",
        "return 0",
        "printf '%s' '${{ github.action }}'",
    ] {
        let mut step = base.clone();
        let StepKind::Shell { run, .. } = &mut step.kind else {
            panic!("preflight is a shell step")
        };
        run[2] = crate::toolchain_env::with_credential_unset_script(body);
        assert!(trusted_shell(&step).is_none(), "accepted {body}");
    }
}
