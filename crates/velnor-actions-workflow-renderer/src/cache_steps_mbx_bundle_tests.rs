use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_EXPORT_CONDITION, MBX_BUNDLE_IMPORT_CONDITION, MBX_BUNDLE_KEY_EXPR,
    MBX_BUNDLE_PATH_EXPR, MBX_BUNDLE_RESTORE_KEYS_EXPR, MBX_BUNDLE_SAVE_CONDITION,
};
use velnor_actions_contract::{Job, JobTimeout, Step, StepId, StepKind, StepRole};

use super::{append_bundle_postludes, mbx_producer_mark_step};
use crate::cache_steps::{CompileDriver, MbxTransport, mbx_steps_for_driver};

const ACTION: &str = "jdx/mr-boxington-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MBX: &str = "1.21.1";
const RUST: &str = "1.98.1";

fn tool_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        (
            "RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), RUST.to_owned()),
    ])
}

fn steps(transport: MbxTransport, working_directory: Option<&str>) -> Vec<Step> {
    mbx_steps_for_driver(
        ACTION,
        CompileDriver::Mbx,
        MBX,
        RUST,
        tool_env(),
        transport,
        working_directory,
    )
    .expect("steps build")
    .expect("mbx steps")
}

fn job(steps: Vec<Step>) -> Job {
    Job {
        services: Vec::new(),
        display_name: "Rust / demo".to_owned(),
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

fn run_of(step: &Step) -> &str {
    let StepKind::Shell { run, .. } = &step.kind else {
        panic!("{} must be a shell step", step.name);
    };
    &run[2]
}

fn with_of(step: &Step) -> &BTreeMap<String, String> {
    let StepKind::Action { with, .. } = &step.kind else {
        panic!("{} must be an action step", step.name);
    };
    with
}

#[test]
fn bundle_prelude_follows_the_ready_check_and_install_only_action() {
    let steps = steps(MbxTransport::SeedWrite, None);
    let roles: Vec<_> = steps.iter().map(|step| step.role).collect();
    assert_eq!(
        roles,
        [
            Some(StepRole::MbxPreflight),
            Some(StepRole::MbxCache),
            Some(StepRole::MbxVersionCheck),
            Some(StepRole::MbxBundleKey),
            Some(StepRole::MbxBundleRestore),
            Some(StepRole::MbxBundleImport),
        ]
    );
    let action = with_of(&steps[1]);
    assert_eq!(action.get("backend").map(String::as_str), Some("local"));
    assert!(!action.contains_key("github-cache-mode"), "{action:?}");
    assert!(!action.contains_key("cache-key"), "{action:?}");
    assert!(!action.contains_key("restore-keys"), "{action:?}");
    assert_eq!(steps[3].id, Some(StepId::MbxBundleKey));
    assert_eq!(steps[4].id, Some(StepId::MbxBundleRestore));
    assert_eq!(
        steps[5].condition.as_deref(),
        Some(MBX_BUNDLE_IMPORT_CONDITION)
    );
}

#[test]
fn install_only_transport_adds_no_bundle_steps() {
    let steps = steps(MbxTransport::InstallOnly, None);
    assert_eq!(steps.len(), 3);
    assert!(
        steps
            .iter()
            .all(|step| !format!("{:?}", step.role).contains("Bundle"))
    );
}

#[test]
fn restore_reads_the_same_compatibility_prefix_and_family_path() {
    let steps = steps(MbxTransport::SeedWrite, Some("backend-rust"));
    let restore = with_of(&steps[4]);
    assert_eq!(
        restore.get("key").map(String::as_str),
        Some(MBX_BUNDLE_KEY_EXPR)
    );
    assert_eq!(
        restore.get("restore-keys").map(String::as_str),
        Some(MBX_BUNDLE_RESTORE_KEYS_EXPR)
    );
    assert_eq!(
        restore.get("path").map(String::as_str),
        Some(CacheFamily::MbxBundle.spec().path_input(None).as_str())
    );
    assert_eq!(MBX_BUNDLE_PATH_EXPR, CacheFamily::MbxBundle.spec().paths[0]);
    // A scoped workspace changes where mbx runs, never the archive path.
    assert!(run_of(&steps[5]).contains("cd \"$GITHUB_WORKSPACE/backend-rust\""));
}

#[test]
fn bundle_key_has_no_run_job_file_or_version_literal() {
    let steps = steps(MbxTransport::SeedWrite, None);
    let script = run_of(&steps[3]);
    for banned in ["github.job", "github.run_", "hashFiles", "ImageVersion"] {
        assert!(!script.contains(banned), "{banned}");
    }
    assert!(script.contains("velnor-v1-mbx-bundle-trusted-"));
    assert!(script.contains("unit=seed"));
    assert!(script.contains("[ \"$trust\" != trusted ] || save=true"));
    let StepKind::Shell { env, .. } = &steps[3].kind else {
        panic!("key step must be a shell step");
    };
    assert_eq!(
        env.get("VELNOR_MBX_BUNDLE_VERSION").map(String::as_str),
        Some(MBX)
    );
    assert_eq!(
        env.get("VELNOR_MBX_BUNDLE_ROOT").map(String::as_str),
        Some(".")
    );
}

#[test]
fn seed_and_suite_bundles_never_share_a_compatibility_digest() {
    let seed_key = run_of(&steps(MbxTransport::SeedWrite, None)[3]).to_owned();
    let suite_key = run_of(&steps(MbxTransport::SuiteBundle, None)[3]).to_owned();
    assert_ne!(seed_key, suite_key);
    assert!(suite_key.contains("unit=suite"));
}

#[test]
fn producer_mark_and_postlude_gate_the_save_on_the_producer_not_later_checks() {
    let mark = mbx_producer_mark_step().expect("mark");
    assert_eq!(mark.role, Some(StepRole::MbxProducerMark));
    assert!(mark.condition.is_none(), "default success() gates the mark");
    assert!(run_of(&mark).contains("mbx-producer-ok"));

    let mut body = steps(MbxTransport::SeedWrite, None);
    body.push(mark);
    let mut jobs = BTreeMap::from([("rust-demo".to_owned(), job(body))]);
    append_bundle_postludes(&mut jobs).expect("postlude");
    let steps = &jobs["rust-demo"].steps;
    let export = &steps[steps.len() - 2];
    let save = &steps[steps.len() - 1];
    assert_eq!(export.role, Some(StepRole::MbxBundleExport));
    assert_eq!(export.id, Some(StepId::MbxBundleExport));
    assert_eq!(
        export.condition.as_deref(),
        Some(MBX_BUNDLE_EXPORT_CONDITION)
    );
    assert!(MBX_BUNDLE_EXPORT_CONDITION.starts_with("always() && !cancelled()"));
    assert!(MBX_BUNDLE_EXPORT_CONDITION.contains("outputs.save == 'true'"));
    assert!(MBX_BUNDLE_EXPORT_CONDITION.contains("cache-hit != 'true'"));
    assert_eq!(save.role, Some(StepRole::MbxBundleSave));
    assert_eq!(save.condition.as_deref(), Some(MBX_BUNDLE_SAVE_CONDITION));
    let save_with = with_of(save);
    assert_eq!(save_with.len(), 2, "a save names no restore keys");
    assert_eq!(
        save_with.get("path").map(String::as_str),
        Some(MBX_BUNDLE_PATH_EXPR)
    );
    let script = run_of(export);
    assert!(script.contains("cache export --group"));
    assert!(script.contains("--format directory"));
    assert!(script.contains("producer_not_succeeded"));
    assert!(script.contains(&format!(
        "-gt {}",
        CacheFamily::MbxBundle.spec().ceiling_bytes / 1024
    )));
}

#[test]
fn readers_share_the_seed_key_and_never_get_an_export() {
    let reader = steps(MbxTransport::SeedRead, None);
    let writer = steps(MbxTransport::SeedWrite, None);
    assert_eq!(
        run_of(&reader[3]),
        run_of(&writer[3]),
        "crate jobs read the entry the seed job writes"
    );
    let mut jobs = BTreeMap::from([("rust-demo".to_owned(), job(reader))]);
    append_bundle_postludes(&mut jobs).expect("reader");
    let roles: Vec<_> = jobs["rust-demo"]
        .steps
        .iter()
        .map(|step| step.role)
        .collect();
    assert!(!roles.contains(&Some(StepRole::MbxBundleExport)));
    assert!(!roles.contains(&Some(StepRole::MbxBundleSave)));
}

#[test]
fn postlude_rejects_a_half_built_lifecycle_and_skips_other_jobs() {
    let mut body = steps(MbxTransport::SeedWrite, None);
    body.push(mbx_producer_mark_step().expect("mark"));
    let mut jobs = BTreeMap::from([("rust-demo".to_owned(), job(body.clone()))]);
    append_bundle_postludes(&mut jobs).expect("first postlude");
    assert!(
        append_bundle_postludes(&mut jobs).is_err(),
        "a second pass must not stack another export"
    );
    body.truncate(3);
    let mut plain = BTreeMap::from([("rust-plain".to_owned(), job(body))]);
    append_bundle_postludes(&mut plain).expect("install-only job");
    assert_eq!(plain["rust-plain"].steps.len(), 3);
}

#[path = "cache_steps_mbx_bundle_runtime_tests.rs"]
mod runtime;
