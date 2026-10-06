use std::collections::BTreeMap;

use velnor_actions_contract::{BuildTask, BuildTaskRunner, Job, PermissionLevel, StepKind};

use crate::MiseSetup;
use crate::verification_jobs::build_task_jobs::{
    BUILD_TASK_DEVELOPER_DIR, BuildTaskArtifact, BuildTaskPolicy, BuildTaskTool,
    CARGO_BINSTALL_ONLY_ENV, INSTALL_BUILD_TASK_BOOTSTRAP_NAME, RUN_BUILD_TASK_NAME,
    VERIFY_BUILD_TASK_MACOS_NAME, VERIFY_BUILD_TASK_MBX_NAME, build_build_task_job,
    validate_build_task_jobs,
};
use crate::verification_jobs::build_task_mise::{
    install_selected_tools_script, run_build_task_script, selected_mise_files,
};

const CHECKOUT: &str = "actions/checkout@0123456789abcdef0123456789abcdef01234567";
const BOLTFFI_MATCHING_REGEX: &str = r"^boltffi-(darwin-aarch64|darwin-x86_64|linux-aarch64(-musl)?|linux-x86_64(-musl)?|windows-arm64|windows-x86_64)\.(tar\.gz|zip)$";

fn artifact(url: &str, checksum: &str) -> BuildTaskArtifact {
    BuildTaskArtifact {
        checksum: format!("sha256:{checksum}"),
        url: url.to_owned(),
        url_api: None,
        signer: None,
        provenance: None,
    }
}

fn tool(
    key: &str,
    version: &str,
    backend: &str,
    artifact: Option<BuildTaskArtifact>,
) -> BuildTaskTool {
    BuildTaskTool {
        key: key.to_owned(),
        version: version.to_owned(),
        backend: backend.to_owned(),
        os: Vec::new(),
        config_options: BTreeMap::new(),
        lock_options: BTreeMap::new(),
        artifact,
    }
}

fn policy() -> BuildTaskPolicy {
    BuildTaskPolicy {
        task: BuildTask {
            id: "native-desktop".to_owned(),
            mise_task: "desktop-ci".to_owned(),
            tools: vec![
                "aqua:nextest-rs/nextest/cargo-nextest".to_owned(),
                "github:boltffi/boltffi".to_owned(),
                "mr-boxington".to_owned(),
                "rust".to_owned(),
                "swiftlint".to_owned(),
                "xcodegen".to_owned(),
            ],
            runner: BuildTaskRunner::Macos26Arm64,
            timeout_minutes: 120,
            cargo_build_jobs: 2,
            nextest_test_threads: 2,
        },
        runner_label: "macos-26".to_owned(),
        mise_setup: MiseSetup {
            uses: "jdx/mise-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
            version: "2026.9.18".to_owned(),
            sha256: "a".repeat(64),
        },
        mise_config_sha256: "b".repeat(64),
        mise_lock_sha256: "c".repeat(64),
        rust_toolchain_sha256: "d".repeat(64),
        selected_tools: selected_tools(),
    }
}

fn selected_tools() -> Vec<BuildTaskTool> {
    vec![
        nextest_tool(),
        boltffi_tool(),
        mbx_tool(),
        rust_tool(),
        swiftlint_tool(),
        xcodegen_tool(),
    ]
}

fn nextest_tool() -> BuildTaskTool {
    tool(
        "aqua:nextest-rs/nextest/cargo-nextest",
        "0.9.140",
        "aqua:nextest-rs/nextest/cargo-nextest",
        Some(artifact(
            "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-0.9.140/cargo-nextest-0.9.140-universal-apple-darwin.tar.gz",
            "58e0a722f9444078fab447783f322acf15a2a771ba785b3fbbe8bacda31c3df9",
        )),
    )
}

fn boltffi_tool() -> BuildTaskTool {
    let options = BTreeMap::from([(
        "matching_regex".to_owned(),
        BOLTFFI_MATCHING_REGEX.to_owned(),
    )]);
    let mut tool = tool(
        "github:boltffi/boltffi",
        "0.30.1",
        "github:boltffi/boltffi",
        Some(artifact(
            "https://github.com/boltffi/boltffi/releases/download/v0.30.1/boltffi-darwin-aarch64.tar.gz",
            "ce3a47b5c398cbb9c327098a612b431f30db15d353d62cee4e4637540fa8321a",
        )),
    );
    tool.config_options = options.clone();
    tool.lock_options = options;
    tool
}

fn mbx_tool() -> BuildTaskTool {
    let mut artifact = artifact(
        "https://github.com/jdx/mr-boxington/releases/download/v1.22.0/mbx-aarch64-apple-darwin.tar.gz",
        "e548b5758498cf822a180b6328597e6aded8fe9bb3046cd918399172ae30dde2",
    );
    artifact.signer = Some(
        "sigstore-oidc:https://github.com/jdx/mr-boxington/.github/workflows/release.yml"
            .to_owned(),
    );
    tool(
        "mr-boxington",
        "1.22.0",
        "packslip:github.com/jdx/mr-boxington",
        Some(artifact),
    )
}

fn rust_tool() -> BuildTaskTool {
    let options = BTreeMap::from([
        ("components".to_owned(), "clippy,rustfmt".to_owned()),
        (
            "targets".to_owned(),
            "aarch64-unknown-linux-gnu,x86_64-unknown-linux-gnu".to_owned(),
        ),
    ]);
    let mut tool = tool("rust", "1.97.1", "core:rust", None);
    tool.config_options = options.clone();
    tool.lock_options = options;
    tool
}

fn swiftlint_tool() -> BuildTaskTool {
    let mut tool = tool(
        "swiftlint",
        "0.65.1",
        "aqua:realm/SwiftLint",
        Some(artifact(
            "https://github.com/realm/SwiftLint/releases/download/0.65.1/portable_swiftlint.zip",
            "c1e429b0599cf1b516f369a2d9ec04eaf0e436f3c12b637df8851fa52ff694d0",
        )),
    );
    tool.os = vec!["macos".to_owned()];
    tool
}

fn xcodegen_tool() -> BuildTaskTool {
    tool(
        "xcodegen",
        "2.46.0",
        "aqua:yonaskolb/XcodeGen",
        Some(artifact(
            "https://github.com/yonaskolb/XcodeGen/releases/download/2.46.0/xcodegen.zip",
            "4d9e34b62172d645eed6457cac13fc222569974098ef4ee9c3368bedf0196806",
        )),
    )
}

fn shell_parts(job: &Job, index: usize) -> Option<(&[String], &BTreeMap<String, String>)> {
    match &job.steps.get(index)?.kind {
        StepKind::Shell { run, env } => Some((run, env)),
        _ => None,
    }
}

#[test]
fn native_job_bootstraps_only_locked_selected_tools_and_guards_current_source() {
    let policy = policy();
    let job = build_build_task_job(&policy, CHECKOUT).expect("native task job");
    assert_eq!(policy.job_id(), "task-native-desktop");
    assert_eq!(job.runs_on, "macos-26");
    assert_eq!(job.timeout_minutes.minutes(), 120);
    assert!(job.needs.is_empty());
    assert!(job.condition.is_none());
    assert!(job.environment.is_none());
    let permissions = job.permissions.as_ref().expect("explicit permissions");
    assert_eq!(permissions.contents, PermissionLevel::Read);
    assert_eq!(permissions.pull_requests, PermissionLevel::None);
    assert_eq!(permissions.actions, PermissionLevel::None);
    assert_eq!(permissions.id_token, PermissionLevel::None);
    assert_eq!(job.steps.len(), 6);

    let StepKind::Action { uses, with, .. } = &job.steps[0].kind else {
        panic!("checkout is not a pinned action");
    };
    assert_eq!(uses, CHECKOUT);
    assert_eq!(
        with.get("persist-credentials").map(String::as_str),
        Some("false")
    );
    assert!(!with.contains_key("repository") && !with.contains_key("ref"));
    assert_eq!(job.steps[1].name, VERIFY_BUILD_TASK_MACOS_NAME);
    let (xcode, env) = shell_parts(&job, 1).expect("Xcode shell step");
    assert!(xcode.join(" ").contains("Xcode 26.6"));
    assert!(xcode.join(" ").contains("17F113"));
    assert!(xcode.join(" ").contains("26.5"));
    assert_eq!(
        env.get("DEVELOPER_DIR").map(String::as_str),
        Some(BUILD_TASK_DEVELOPER_DIR)
    );
    assert!(matches!(&job.steps[2].kind, StepKind::Action { .. }));

    assert_eq!(job.steps[3].name, INSTALL_BUILD_TASK_BOOTSTRAP_NAME);
    let (bootstrap, env) = shell_parts(&job, 3).expect("bootstrap shell step");
    let bootstrap = bootstrap.join(" ");
    assert!(bootstrap.contains("mise --no-env --locked --no-hooks install --jobs 2"));
    assert!(bootstrap.contains("export MISE_NO_ENV=1"));
    assert!(bootstrap.contains("MISE_CARGO_BINSTALL_ONLY=1"));
    assert!(bootstrap.contains("[[tools.\"mr-boxington\"]]"));
    assert!(bootstrap.contains("[[tools.\"rust\"]]"));
    assert!(!bootstrap.contains("cargo:boltffi_cli"));
    assert!(!bootstrap.contains("cargo install"));
    assert!(bootstrap.contains("/velnor-task-"));
    assert_eq!(
        env.get(CARGO_BINSTALL_ONLY_ENV).map(String::as_str),
        Some("1")
    );

    assert_eq!(job.steps[4].name, VERIFY_BUILD_TASK_MBX_NAME);
    let (guard, _) = shell_parts(&job, 4).expect("source guard shell step");
    let guard = guard.join(" ");
    assert!(guard.contains("wrappers.cargo.command"));
    assert!(guard.contains("MBX_CARGO_SHIM_MODE"));
    assert!(guard.contains("mise.lock"));
    assert!(guard.contains("rust-toolchain.toml"));
    assert!(guard.contains("MISE_DATA_DIR/command-wrappers/bin/cargo"));
    assert!(guard.contains("test -L \"$cargo_path\""));
    assert!(guard.contains("/usr/bin/readlink \"$cargo_path\""));
    assert!(guard.contains("mise --no-env --locked --no-hooks exec -- cargo --version"));

    assert_eq!(job.steps[5].name, RUN_BUILD_TASK_NAME);
    let (run, env) = shell_parts(&job, 5).expect("task shell step");
    let run = run.join(" ");
    assert!(run.contains("mise --no-env --locked --no-hooks run --skip-tools desktop-ci"));
    assert!(run.contains("export MISE_NO_ENV=1"));
    assert_eq!(env.get("CARGO_BUILD_JOBS").map(String::as_str), Some("2"));
    assert_eq!(
        env.get("NEXTEST_TEST_THREADS").map(String::as_str),
        Some("2")
    );
    assert_eq!(
        env.get("DEVELOPER_DIR").map(String::as_str),
        Some(BUILD_TASK_DEVELOPER_DIR)
    );
}

#[test]
fn env_source_overlay_is_suppressed_before_config_inspection_and_task_execution() {
    let overlay_fixture = "[env]\n_.source = \"./sentinel.sh\"\n";
    assert!(overlay_fixture.contains("_.source"));

    let policy = policy();
    let bootstrap = install_selected_tools_script(&policy).expect("bootstrap script");
    let env_disable = bootstrap
        .find("export MISE_NO_ENV=1")
        .expect("no-env export");
    let config_inspection = bootstrap
        .find("mise --no-env --no-hooks config ls --json")
        .expect("no-env config inspection");
    assert!(env_disable < config_inspection, "{bootstrap}");

    let run = run_build_task_script(&policy).expect("run script");
    let env_disable = run.find("export MISE_NO_ENV=1").expect("no-env export");
    let task = run
        .find("mise --no-env --locked --no-hooks run --skip-tools desktop-ci")
        .expect("no-env task run");
    assert!(env_disable < task, "{run}");
    assert!(run.contains("unset MISE_CONFIG_FILE MISE_ENV MISE_ENV_FILE"));
}

#[test]
fn selected_config_and_lock_preserve_only_selected_platform_pins() {
    let (config, lock) = selected_mise_files(&policy()).expect("selected source projection");
    assert!(config.contains("[tools.\"mr-boxington\"]\nversion = \"1.22.0\""));
    assert!(config.contains("[tools.\"rust\"]\nversion = \"1.97.1\""));
    assert!(config.contains("components = \"clippy,rustfmt\""));
    assert!(config.contains("targets = \"aarch64-unknown-linux-gnu,x86_64-unknown-linux-gnu\""));
    assert!(config.contains("\"github:boltffi/boltffi\""));
    assert!(config.contains(BOLTFFI_MATCHING_REGEX));
    assert!(config.contains("os = [\"macos\"]"));
    assert!(config.contains("command = \"mbx\""));
    assert!(lock.contains("backend = \"packslip:github.com/jdx/mr-boxington\""));
    assert!(lock.contains("sha256:"));
    assert!(lock.contains("platforms.macos-arm64"));
    assert!(lock.contains("boltffi-darwin-aarch64.tar.gz"));
    assert!(!config.contains("codebook-lsp"));
    assert!(!lock.contains("cargo:boltffi_cli"));
}

#[test]
fn unsupported_source_backend_and_unbound_artifacts_fail_closed() {
    let mut invalid_policy = policy();
    let cargo = BuildTaskTool {
        key: "cargo:unsafe-tool".to_owned(),
        version: "1.0.0".to_owned(),
        backend: "cargo:unsafe-tool".to_owned(),
        os: Vec::new(),
        config_options: BTreeMap::new(),
        lock_options: BTreeMap::new(),
        artifact: Some(artifact(
            "https://github.com/example/tool/releases/download/v1.0/tool-macos-arm64.tar.gz",
            &"e".repeat(64),
        )),
    };
    invalid_policy.task.tools = vec![
        "cargo:unsafe-tool".to_owned(),
        "mr-boxington".to_owned(),
        "rust".to_owned(),
    ];
    let mbx = invalid_policy
        .selected_tools
        .iter()
        .find(|tool| tool.key == "mr-boxington")
        .cloned()
        .expect("MBX tool");
    let rust = invalid_policy
        .selected_tools
        .iter()
        .find(|tool| tool.key == "rust")
        .cloned()
        .expect("Rust tool");
    invalid_policy.selected_tools = vec![cargo, mbx, rust];
    assert!(selected_mise_files(&invalid_policy).is_err());

    let mut bad_asset_policy = policy();
    bad_asset_policy.selected_tools[0]
        .artifact
        .as_mut()
        .expect("asset")
        .url = "https://evil.example/tool.tar.gz".to_owned();
    assert!(install_selected_tools_script(&bad_asset_policy).is_err());
    assert!(build_build_task_job(&bad_asset_policy, CHECKOUT).is_err());
}

#[test]
fn declared_jobs_are_exact_sorted_and_unique() {
    let policy = policy();
    let id = policy.job_id();
    let job = build_build_task_job(&policy, CHECKOUT).expect("native task job");
    let jobs = BTreeMap::from([(id.clone(), job.clone())]);
    assert_eq!(
        validate_build_task_jobs(&jobs, std::slice::from_ref(&policy), CHECKOUT)
            .expect("declared native job")
            .as_slice(),
        std::slice::from_ref(&id)
    );

    let mut changed = jobs;
    changed.insert(
        id,
        Job {
            timeout_minutes: velnor_actions_contract::JobTimeout::new(119).expect("timeout"),
            ..job
        },
    );
    assert!(validate_build_task_jobs(&changed, &[policy], CHECKOUT).is_err());
}

#[test]
fn emitted_scripts_are_single_line_without_command_substitution() {
    let job = build_build_task_job(&policy(), CHECKOUT).expect("native task job");
    for step in &job.steps {
        let StepKind::Shell { run, .. } = &step.kind else {
            continue;
        };
        for arg in run {
            assert!(!arg.contains('\n'), "single-line script: {arg}");
            assert!(!arg.contains("$("), "no substitution: {arg}");
            assert!(!arg.contains('`'), "no backticks: {arg}");
        }
    }
    let bootstrap = install_selected_tools_script(&policy()).expect("bootstrap script");
    assert!(bootstrap.contains("printf '%s\\n'"));
    assert!(!bootstrap.contains("<<'VELNOR_SELECTED_MISE_CONFIG'"));
}
