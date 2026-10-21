#[path = "cache_doctor_qualification_tests.rs"]
mod qualification_tests;

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, SOURCES_KEY_PLAN_EXPR, SOURCES_KEY_STEP_EXPR, SOURCES_RESTORE_KEYS_PLAN_EXPR,
    SOURCES_RESTORE_KEYS_STEP_EXPR, SOURCES_SAVE_CONDITION,
};
use velnor_actions_contract::workflow::{
    QualificationCacheLayer, QualificationCacheStepId, QualificationCacheStepStage,
};
use velnor_actions_contract::{Job, JobTimeout, PullRequestCachePolicy, Step, StepKind};

use super::{MBX_OBJECTS_MODE_DOCTOR_EXEMPTION, check};

const RESTORE: &str = "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
const SAVE: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";

fn action(name: &str, uses: &str, with: &[(&str, &str)], condition: Option<&str>) -> Step {
    Step {
        parallel: false,
        name: name.to_owned(),
        id: None,
        role: None,
        condition: condition.map(str::to_owned),
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: with
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
            env: BTreeMap::new(),
        },
    }
}

fn job(steps: Vec<Step>) -> Job {
    Job {
        services: Vec::new(),
        display_name: "Job".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    }
}

fn sources_paths() -> String {
    CacheFamily::Sources.spec().path_input(None)
}

fn tools_save(condition: &str) -> Step {
    let paths = crate::cache_steps::TOOLS_CACHE_PATHS.join("\n");
    action(
        "Save Mise tools",
        SAVE,
        &[
            ("key", crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION),
            ("path", &paths),
        ],
        Some(condition),
    )
}

/// A valid Plan writer plus one crate reader.
fn valid() -> BTreeMap<String, Job> {
    let paths = sources_paths();
    let tools = crate::cache_p08::tools_cache_save_condition(PullRequestCachePolicy::ReadOnly);
    let plan = job(vec![
        action(
            "Restore Cargo sources",
            RESTORE,
            &[
                ("key", SOURCES_KEY_STEP_EXPR),
                ("path", &paths),
                ("restore-keys", SOURCES_RESTORE_KEYS_STEP_EXPR),
            ],
            None,
        ),
        action(
            "Save Cargo sources",
            SAVE,
            &[("key", SOURCES_KEY_STEP_EXPR), ("path", &paths)],
            Some(SOURCES_SAVE_CONDITION),
        ),
        tools_save(&tools),
    ]);
    let reader = job(vec![action(
        "Restore Cargo sources",
        RESTORE,
        &[
            ("key", SOURCES_KEY_PLAN_EXPR),
            ("path", &paths),
            ("restore-keys", SOURCES_RESTORE_KEYS_PLAN_EXPR),
        ],
        None,
    )]);
    BTreeMap::from([("plan".to_owned(), plan), ("rust-a".to_owned(), reader)])
}

fn with_reader(step: Step) -> BTreeMap<String, Job> {
    let mut jobs = valid();
    jobs.get_mut("rust-a").expect("reader").steps.push(step);
    jobs
}

fn rejects(jobs: &BTreeMap<String, Job>, rule: &str) {
    let err = check(jobs).expect_err(rule).to_string();
    assert!(err.contains(rule), "{rule}: {err}");
}

#[test]
fn doctor_accepts_the_generated_family_shapes() {
    check(&valid()).expect("valid workflow");
}

#[test]
fn i1_rejects_unstable_or_version_bound_keys() {
    let paths = sources_paths();
    for (key, rule) in [
        (
            "velnor-v2-sources-${{ github.job }}",
            "i1_key_contains:github.job",
        ),
        (
            "velnor-v2-sources-${{ github.workflow }}",
            "i1_key_contains:github.workflow",
        ),
        (
            "velnor-v2-sources-${{ github.run_id }}",
            "i1_key_contains:github.run_",
        ),
        (
            "velnor-v2-sources-${{ hashFiles('Cargo.lock') }}",
            "i1_key_contains:hashFiles(",
        ),
        (
            "velnor-v2-sources-ImageVersion",
            "i1_key_contains:ImageVersion",
        ),
        ("velnor-v2-sources-1.99.1", "i1_key_contains_semver"),
        (
            "velnor-v2-sources-55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
            "i1_key_contains_action_sha",
        ),
    ] {
        let step = action(
            "Restore",
            RESTORE,
            &[("key", key), ("path", &paths), ("restore-keys", "")],
            None,
        );
        rejects(&with_reader(step), rule);
    }
}

#[test]
fn i1_rejects_unknown_families_and_overlong_keys() {
    let step = action("Restore", RESTORE, &[("key", "velnor-v1-sources-x")], None);
    rejects(&with_reader(step), "unknown_family");
    let long = format!("velnor-v2-sources-{}", "x".repeat(600));
    let paths = sources_paths();
    let step = action(
        "Restore",
        RESTORE,
        &[("key", &long), ("path", &paths)],
        None,
    );
    rejects(&with_reader(step), "key_too_long");
}

#[test]
fn i2_rejects_a_path_list_not_taken_from_the_spec() {
    let mut jobs = valid();
    let reader = jobs.get_mut("rust-a").expect("reader");
    reader.steps[0] = action(
        "Restore Cargo sources",
        RESTORE,
        &[
            ("key", SOURCES_KEY_PLAN_EXPR),
            ("path", "${{ runner.temp }}/velnor/cargo/registry/index"),
            ("restore-keys", SOURCES_RESTORE_KEYS_PLAN_EXPR),
        ],
        None,
    );
    rejects(&jobs, "i2_path_not_from_spec");
}

#[test]
fn i3_rejects_sources_paths_in_another_family() {
    let tools = crate::cache_p08::tools_cache_save_condition(PullRequestCachePolicy::ReadOnly);
    let mut save = tools_save(&tools);
    let StepKind::Action { with, .. } = &mut save.kind else {
        unreachable!("action");
    };
    with.insert(
        "path".to_owned(),
        "${{ runner.temp }}/velnor/cargo/registry".to_owned(),
    );
    rejects(&with_reader(save), "i2_path_not_from_spec");
    let shared = super::check_sources_isolation(&BTreeMap::from([(
        "${{ runner.temp }}/velnor/cargo/registry".to_owned(),
        super::Family::Tools,
    )]))
    .expect_err("nested sources path");
    assert!(
        shared.to_string().contains("i3_sources_path_shared"),
        "{shared}"
    );
}

#[test]
fn i7_rejects_ungated_or_misplaced_saves() {
    let paths = sources_paths();
    let push_only = action(
        "Save Cargo sources",
        SAVE,
        &[("key", SOURCES_KEY_PLAN_EXPR), ("path", &paths)],
        Some("success() && github.event_name == 'push'"),
    );
    rejects(&with_reader(push_only), "i7_save_not_trust_gated");
    let reader_save = action(
        "Save Cargo sources",
        SAVE,
        &[("key", SOURCES_KEY_PLAN_EXPR), ("path", &paths)],
        Some(SOURCES_SAVE_CONDITION),
    );
    rejects(&with_reader(reader_save), "i7_save_not_trust_gated");
    let tools = tools_save("success() && github.event_name == 'push'");
    rejects(&with_reader(tools), "i7_save_not_trust_gated");
    let combined = action(
        "Cache",
        "actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
        &[("key", SOURCES_KEY_PLAN_EXPR), ("path", &paths)],
        None,
    );
    rejects(
        &with_reader(combined),
        "i7_combined_cache_action_saves_ungated",
    );
}

#[test]
fn only_the_named_mbx_objects_mode_exemption_skips_the_doctor() {
    let mbx =
        format!("{MBX_OBJECTS_MODE_DOCTOR_EXEMPTION}47ce49b32025d93a7a2f4570815afaa2b7092240");
    let key = "velnor-mbx-1.26.0-job-${{ github.job }}";
    check(&with_reader(action(
        "Set up MBX",
        &mbx,
        &[("cache-key", key)],
        None,
    )))
    .expect("exempt");
    let other = action(
        "Other",
        "owner/other-cache@47ce49b32025d93a7a2f4570815afaa2b7092240",
        &[("cache-key", key)],
        None,
    );
    rejects(&with_reader(other), "i1_key_contains:github.job");
}

#[test]
fn tools_identity_binds_resolved_versions_never_hashed_files() {
    let identity = |d: &str| {
        let mut step = action(
            "V2 identity",
            "./.github/actions/velnor-tools-prelude-u26",
            &[("d", d)],
            None,
        );
        step.role = Some(velnor_actions_contract::StepRole::ToolsCacheIdentity);
        step
    };
    let digest = "a".repeat(64);
    let resolved = format!("{digest}-${{VELNOR_TOOL_RUST}}-${{VELNOR_TOOL_RUST_LOCK}}");
    check(&with_reader(identity(&resolved))).expect("resolved-version identity");
    rejects(
        &with_reader(identity(&format!(
            "{digest}-${{{{ hashFiles('mise.toml','mise.lock') }}}}"
        ))),
        "i1_tools_identity_contains:hashFiles(",
    );
    rejects(
        &with_reader(identity(&format!("{digest}-${{VELNOR_TOOL_RUST}}"))),
        "i1_tools_identity_shape",
    );
}

#[test]
fn bun_families_fail_closed_until_a_generator_emits_them() {
    for (family, path) in [
        ("dbt-packages", "${{ runner.temp }}/velnor/dbt-packages/x"),
        ("python-venv", "${{ runner.temp }}/velnor/uv-cache"),
        (
            "ansible-collections",
            "${{ runner.temp }}/velnor/ansible-collections",
        ),
        ("bun-install", "${{ runner.temp }}/velnor/bun-cache"),
        (
            "playwright-browsers",
            "${{ runner.temp }}/velnor/ms-playwright",
        ),
    ] {
        let key = format!("velnor-v1-{family}-trusted-x-y");
        for (uses, condition) in [
            (RESTORE, None),
            (
                SAVE,
                Some("success() && needs.plan.outputs.cache_trust == 'trusted'"),
            ),
        ] {
            let step = action("Cache", uses, &[("key", &key), ("path", path)], condition);
            let jobs = with_reader(step);
            let text = check(&jobs).expect_err(family).to_string();
            assert!(
                text.contains("i2_path_not_from_spec") || text.contains("i7_save_not_trust_gated"),
                "{family}: {text}"
            );
        }
    }
}
