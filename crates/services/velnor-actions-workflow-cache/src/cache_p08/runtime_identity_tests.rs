use super::*;
use std::fs;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use velnor_actions_contract_workflow::{JobTimeout, StepRole};

const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const MISE: &str = "jdx/mise-action@0123456789abcdef0123456789abcdef01234567";
const HOSTED_LABEL: &str = "ubuntu-26.04";
const HOSTED_TARGET: &str = "x86_64-unknown-linux-gnu";

fn setup() -> MiseSetup {
    MiseSetup {
        uses: MISE.to_owned(),
        version: "2026.9.18".to_owned(),
        sha256: "a".repeat(64),
    }
}

fn mise_job(runs_on: &str) -> Job {
    Job {
        display_name: "Cache identity fixture".to_owned(),
        runs_on: runs_on.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            velnor_actions_workflow_steps::steps::checkout_step(CHECKOUT).expect("checkout"),
            velnor_actions_workflow_steps::steps::shell_step(
                "Install Rust",
                vec![
                    "mise".to_owned(),
                    "install".to_owned(),
                    "rust@1.98.1".to_owned(),
                ],
                BTreeMap::new(),
            )
            .expect("mise command"),
        ],
    }
}

#[test]
fn hosted_setup_uses_runtime_image_identity_and_is_idempotent() {
    let setup = setup();
    let mut job = mise_job(HOSTED_LABEL);
    ensure_setup_p08("hosted", &mut job, &setup, false, HOSTED_TARGET, CHECKOUT)
        .expect("hosted cache inserted");

    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Checkout",
            CACHE_IDENTITY_STEP_NAME,
            crate::tool_seed::TOOL_SEED_NAME,
            "Setup Mise",
            "Install Rust",
        ]
    );
    let StepKind::Action { with, .. } = &job.steps[3].kind else {
        panic!("Setup Mise must be an action")
    };
    assert_eq!(
        with.get("cache").map(String::as_str),
        Some(crate::cache_p08::MISE_CACHE_ENABLED_EXPR)
    );
    let expected_key = crate::cache_p08::mise_cache_key_for_tools(
        "ubuntu26",
        HOSTED_TARGET,
        "2026.9.18",
        &["rust@1.98.1".to_owned()],
    )
    .expect("derived key");
    assert_eq!(
        with.get("cache_key").map(String::as_str),
        Some(expected_key.as_str())
    );
    assert!(expected_key.starts_with("mise-v2-hosted-ubuntu26-x86_64-unknown-linux-gnu-"));
    assert!(expected_key.ends_with("-${{env.VELNOR_MISE_CACHE_SUFFIX}}"));
    assert_eq!(job.steps[2].role, Some(StepRole::ToolSeed));

    let before = job.clone();
    ensure_setup_p08("hosted", &mut job, &setup, false, HOSTED_TARGET, CHECKOUT)
        .expect("repeat finalization is stable");
    assert_eq!(job.steps, before.steps);
}

#[test]
fn scale_set_and_unrecognized_hosted_labels_cannot_use_the_cache() {
    let setup = setup();
    for (runs_on, target) in [
        ("scale-set:velnor+orbstack-linux", HOSTED_TARGET),
        (HOSTED_LABEL, "aarch64-apple-darwin"),
        ("macos-15-arm64", "aarch64-apple-darwin"),
        ("macos-26", "aarch64-apple-darwin"),
    ] {
        let mut job = mise_job(runs_on);
        ensure_setup_p08("uncached", &mut job, &setup, false, target, CHECKOUT)
            .expect("unsupported runtime stays uncached");
        assert!(!job.steps.iter().any(is_runtime_identity_candidate));
        assert!(
            !job.steps
                .iter()
                .any(|step| step.role == Some(StepRole::ToolSeed))
        );
        let setup_step = job
            .steps
            .iter()
            .find(|step| is_setup_step(step))
            .expect("Mise setup remains present");
        let StepKind::Action { with, .. } = &setup_step.kind else {
            panic!("Mise setup must be an action")
        };
        assert_eq!(with.get("cache").map(String::as_str), Some("false"));
        assert!(!with.contains_key("cache_key"));
        let mut jobs = BTreeMap::from([("job".to_owned(), job)]);
        crate::cache_elect::elect_mise_cache_writers(&mut jobs)
            .expect("uncached job has no writer");
        assert!(
            !jobs["job"]
                .steps
                .iter()
                .any(|step| step.role == Some(StepRole::ToolsCacheSave))
        );
    }
}

#[test]
fn malformed_or_duplicate_runtime_identity_steps_fail_closed() {
    let setup = setup();
    let mut job = mise_job(HOSTED_LABEL);
    ensure_setup_p08("hosted", &mut job, &setup, false, HOSTED_TARGET, CHECKOUT)
        .expect("hosted cache inserted");
    let identity_at = job
        .steps
        .iter()
        .position(|step| step.name == CACHE_IDENTITY_STEP_NAME)
        .expect("identity step");
    job.steps[identity_at].condition = Some("always()".to_owned());
    assert!(ensure_setup_p08("mutated", &mut job, &setup, false, HOSTED_TARGET, CHECKOUT).is_err());
}

#[test]
fn identity_probe_binds_provider_os_arch_and_image_version() {
    let hosted = run_identity_probe("github-hosted", "Linux", "X64", "ubuntu26", "20261008.1");
    assert!(hosted.contains("VELNOR_MISE_CACHE_ENABLED=true\n"));
    assert!(hosted.contains("VELNOR_MISE_CACHE_SUFFIX=ubuntu26-20261008.1\n"));

    for (provider, os, arch, image_os, version) in [
        ("self-hosted", "Linux", "X64", "ubuntu26", "20261008.1"),
        ("github-hosted", "Linux", "ARM64", "ubuntu26", "20261008.1"),
        ("github-hosted", "Linux", "X64", "ubuntu24", "20261008.1"),
        ("github-hosted", "Linux", "X64", "ubuntu26", ""),
        (
            "github-hosted",
            "Linux",
            "X64",
            "ubuntu26",
            "20261008.1$(false)",
        ),
    ] {
        let disabled = run_identity_probe(provider, os, arch, image_os, version);
        assert!(disabled.contains("VELNOR_MISE_CACHE_ENABLED=false\n"));
        assert!(disabled.contains("VELNOR_MISE_CACHE_SUFFIX=disabled\n"));
    }
}

#[test]
fn runtime_image_version_changes_the_real_cache_key_suffix() {
    let one = run_identity_probe("github-hosted", "Linux", "X64", "ubuntu26", "20261008.1");
    let two = run_identity_probe("github-hosted", "Linux", "X64", "ubuntu26", "20261009.1");
    assert_ne!(suffix(&one), suffix(&two));
    assert!(suffix(&one).starts_with("ubuntu26-"));
    assert!(suffix(&two).starts_with("ubuntu26-"));

    let template = crate::cache_p08::mise_cache_key_for_tools(
        "ubuntu26",
        HOSTED_TARGET,
        "2026.9.18",
        &["rust@1.98.1".to_owned()],
    )
    .expect("cache key template");
    let one_key = template.replace(crate::cache_p08::MISE_CACHE_SUFFIX_EXPR, suffix(&one));
    let two_key = template.replace(crate::cache_p08::MISE_CACHE_SUFFIX_EXPR, suffix(&two));
    assert_ne!(one_key, two_key);
}

#[test]
fn different_hosted_image_families_have_different_writer_keys() {
    let ubuntu = crate::cache_p08::mise_cache_key_for_tools(
        "ubuntu26",
        HOSTED_TARGET,
        "2026.9.18",
        &["rust@1.98.1".to_owned()],
    )
    .expect("Ubuntu cache key");
    let macos = crate::cache_p08::mise_cache_key_for_tools(
        "macos15",
        "aarch64-apple-darwin",
        "2026.9.18",
        &["rust@1.98.1".to_owned()],
    )
    .expect("macOS cache key");
    assert_ne!(ubuntu, macos);
    assert!(ubuntu.starts_with("mise-v2-hosted-ubuntu26-"));
    assert!(macos.starts_with("mise-v2-hosted-macos15-"));
}

fn suffix(env: &str) -> &str {
    env.lines()
        .find_map(|line| line.strip_prefix("VELNOR_MISE_CACHE_SUFFIX="))
        .expect("runtime suffix")
}

fn run_identity_probe(
    provider: &str,
    os: &str,
    arch: &str,
    image_os: &str,
    image_version: &str,
) -> String {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "velnor-cache-identity-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).expect("probe temp directory");
    let github_env = root.join("github_env");
    fs::write(&github_env, "").expect("runner env file");
    let output = Command::new("bash")
        .arg("-c")
        .arg(CACHE_IDENTITY_SCRIPT)
        .env("GITHUB_ENV", &github_env)
        .env(RUNNER_ENVIRONMENT_KEY, provider)
        .env(RUNNER_OS_KEY, os)
        .env(RUNNER_ARCH_KEY, arch)
        .env(EXPECTED_IMAGE_OS_KEY, "ubuntu26")
        .env(EXPECTED_RUNNER_OS_KEY, "Linux")
        .env(EXPECTED_RUNNER_ARCH_KEY, "X64")
        .env("ImageOS", image_os)
        .env("ImageVersion", image_version)
        .output()
        .expect("run identity probe");
    assert!(output.status.success(), "probe failed: {output:?}");
    let result = fs::read_to_string(&github_env).expect("read runner env file");
    fs::remove_dir_all(root).expect("remove probe directory");
    result
}
