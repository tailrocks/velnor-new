use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout, PullRequestCachePolicy, Step, StepRole};

use super::{RenderContext, insert_verified_cache_helper};

const RUNS_ON: &str = "ubuntu-26.04";
const CHECKOUT: &str = "actions/checkout@0000000000000000000000000000000000000000";

fn helper(sha256: &str) -> Step {
    let staged = format!("{}0.1.0", crate::steps::STAGED_BINARY_PREFIX);
    let env = BTreeMap::from([
        (
            crate::steps::ASSET_URL_ENV.to_owned(),
            "https://example.invalid/velnor".to_owned(),
        ),
        (crate::steps::ASSET_SHA_ENV.to_owned(), sha256.to_owned()),
        (crate::steps::RELEASE_COMMIT_ENV.to_owned(), "b".repeat(40)),
    ]);
    crate::steps::acquire_velnor_step(
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "curl -fsSL \"$VELNOR_ASSET_URL\" -o {staged} && echo \"$VELNOR_ASSET_SHA256  {staged}\" | sha256sum -c - && chmod +x {staged}"
            ),
        ],
        &env,
    )
    .expect("release-bound acquisition step")
}

fn context(expected: Option<Step>, required: bool) -> RenderContext {
    RenderContext {
        generator_version: "0.1.0".to_owned(),
        report_helper_version: "0.1.0".to_owned(),
        runs_on: RUNS_ON.to_owned(),
        scale_set_selector: None,
        scale_set_image_source_sha: None,
        staged_binary: format!("{}0.1.0", crate::steps::STAGED_BINARY_PREFIX),
        verified_cache_helpers: expected.map_or_else(BTreeMap::new, |step| {
            BTreeMap::from([(RUNS_ON.to_owned(), vec![step])])
        }),
        require_verified_cache_helper: required,
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: CHECKOUT.to_owned(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: PullRequestCachePolicy::ReadOnly,
        mise_quiet: true,
        plan_consumer_env: BTreeMap::new(),
    }
}

fn job(steps: Vec<Step>) -> Job {
    Job {
        display_name: "Cache helper".to_owned(),
        runs_on: RUNS_ON.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    }
}

fn checkout() -> Step {
    crate::steps::checkout_step(CHECKOUT).expect("pinned checkout")
}

#[test]
fn inserts_the_helper_from_the_exact_runner_map() {
    let expected = helper(&"a".repeat(64));
    let context = context(Some(expected.clone()), true);
    let mut job = job(vec![checkout()]);

    insert_verified_cache_helper("consumer", &mut job, &context).expect("mapped helper");

    assert_eq!(job.steps, [checkout(), expected]);
}

#[test]
fn rejects_a_different_acquire_step_even_when_its_role_is_present() {
    let expected = helper(&"a".repeat(64));
    let mut wrong = helper(&"c".repeat(64));
    wrong.name = expected.name.clone();
    let context = context(Some(expected), true);
    let mut job = job(vec![checkout(), wrong]);

    let error = insert_verified_cache_helper("consumer", &mut job, &context)
        .expect_err("the mapped release digest must match");

    assert!(format!("{error:?}").contains("cache_helper_map_mismatch:consumer"));
}

#[test]
fn required_context_rejects_an_existing_helper_without_a_runner_map() {
    let mut job = job(vec![checkout(), helper(&"a".repeat(64))]);

    let error = insert_verified_cache_helper("consumer", &mut job, &context(None, true))
        .expect_err("required helper must be mapped");

    assert!(format!("{error:?}").contains("cache_helper_unmapped:consumer"));
}

#[test]
fn bootstrap_context_without_a_helper_keeps_the_job_cold() {
    let mut job = job(vec![checkout()]);

    insert_verified_cache_helper("bootstrap", &mut job, &context(None, false))
        .expect("bootstrap has no helper requirement");

    assert_eq!(job.steps, [checkout()]);
    assert!(
        job.steps
            .iter()
            .all(|step| step.role != Some(StepRole::AcquireVelnor))
    );
}

#[test]
fn rejects_malformed_runner_map_entries() {
    let mut expected = helper(&"a".repeat(64));
    expected.role = None;
    let context = context(Some(expected), true);
    let mut job = job(vec![checkout()]);

    let error = insert_verified_cache_helper("consumer", &mut job, &context)
        .expect_err("the helper map step needs a trusted helper role");

    assert!(format!("{error:?}").contains("cache_helper_map_malformed:consumer"));
}
