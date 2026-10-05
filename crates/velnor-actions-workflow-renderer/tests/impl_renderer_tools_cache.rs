//! Behavioral checks for cache compatibility and the cold fallback.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_actions_contract::StepKind;
use velnor_actions_workflow_renderer::cache_p08::tools_cache_key_for_tools;
use velnor_actions_workflow_renderer::steps::{
    TOOLS_CACHE_ELIGIBLE_ENV, TOOLS_CACHE_PATHS, TOOLS_CACHE_RESTORE_CONDITION,
    TOOLS_CACHE_SAVE_CONDITION, TOOLS_IMAGE_IDENTITY_NAME, TOOLS_IMAGE_OS_ENV,
    TOOLS_IMAGE_VERSION_ENV, TOOLS_MISE_BOOTSTRAP_BINARY, TOOLS_MISE_DATA_DIR, TOOLS_RESTORE_USES,
    TOOLS_SAVE_USES, is_tools_cache_key, tools_cache_image_identity_step, tools_cache_path_input,
    tools_restore_step, tools_save_step,
};

use super::impl_renderer_fixtures::MISE_SHA256;

#[test]
fn image_identity_helper_keeps_unknown_runners_on_cold_path() {
    let step = tools_cache_image_identity_step().expect("identity step");
    assert_eq!(step.name, TOOLS_IMAGE_IDENTITY_NAME);
    let StepKind::Shell { run, .. } = step.kind else {
        panic!("image identity must be a shell step");
    };
    assert_eq!(run.len(), 3);
    assert_eq!(run[0], "bash");
    assert_eq!(run[1], "-c");
    let script = &run[2];

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "velnor image identity {} {unique}",
        std::process::id()
    ));
    fs::create_dir(&directory).expect("create isolated temp directory");
    let env_file = directory.join("github env");

    let valid = run_identity(script, &env_file, Some("ubuntu24.04"), Some("20260928.1"));
    assert_eq!(
        valid,
        format!(
            "{TOOLS_IMAGE_OS_ENV}=ubuntu24.04\n{TOOLS_IMAGE_VERSION_ENV}=20260928.1\n{TOOLS_CACHE_ELIGIBLE_ENV}=true\n"
        )
    );

    let invalid = [
        (None, Some("20260928.1")),
        (Some("ubuntu24.04"), None),
        (Some(""), Some("20260928.1")),
        (Some("ubuntu24.04"), Some("")),
        (Some("unknown"), Some("20260928.1")),
        (Some("ubuntu24.04"), Some("unobserved")),
        (Some("ubuntu24.04"), Some("stable")),
        (Some("."), Some("20260928.1")),
        (Some("ubuntu24.04"), Some("-")),
        (Some("self-hosted"), Some("20260928.1")),
        (Some("ubuntu/24.04"), Some("20260928.1")),
        (Some("ubuntu 24.04"), Some("20260928.1")),
        (Some("ubuntu\n24.04"), Some("20260928.1")),
        (Some("${{ github.ref }}"), Some("20260928.1")),
    ];
    let disabled = format!(
        "{TOOLS_IMAGE_OS_ENV}=unknown\n{TOOLS_IMAGE_VERSION_ENV}=unknown\n{TOOLS_CACHE_ELIGIBLE_ENV}=false\n"
    );
    for (image_os, image_version) in invalid {
        assert_eq!(
            run_identity(script, &env_file, image_os, image_version),
            disabled,
            "invalid metadata must overwrite inherited eligibility and emit no raw values"
        );
    }
    let no_github_env = Command::new("bash")
        .args(["-c", script])
        .env_remove("GITHUB_ENV")
        .env(TOOLS_CACHE_ELIGIBLE_ENV, "true")
        .status()
        .expect("run helper without GITHUB_ENV");
    assert!(
        !no_github_env.success(),
        "missing output channel must fail closed instead of retaining inherited eligibility"
    );
    fs::remove_file(&env_file).expect("remove runner env fixture");
    fs::remove_dir(&directory).expect("remove isolated temp directory");
}

#[test]
fn tool_cache_actions_share_exact_pinned_identity_and_payload() {
    let selectors = ["rust@1.98.1".to_owned(), "actionlint@1.7.12".to_owned()];
    let key = tools_cache_key_for_tools(
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        MISE_SHA256,
        &selectors,
    )
    .expect("canonical key");
    assert!(
        is_tools_cache_key(&key),
        "generated key is canonical: {key}"
    );
    let equivalent_union = tools_cache_key_for_tools(
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        MISE_SHA256,
        &["actionlint@1.7.12".to_owned(), "rust@1.98.1".to_owned()],
    )
    .expect("canonical reordered key");
    assert_eq!(key, equivalent_union, "selector union order is canonical");
    let rust_only = tools_cache_key_for_tools(
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        MISE_SHA256,
        &["rust@1.98.1".to_owned()],
    )
    .expect("Rust-only key");
    assert_ne!(key, rust_only, "different tool inventory cannot restore");
    assert!(
        tools_cache_key_for_tools(
            "x86_64-unknown-linux-gnu",
            "2026.9.16",
            "bad-sha",
            &selectors,
        )
        .is_err(),
        "Mise bootstrap requires exact binary digest"
    );
    let other_mise_binary = tools_cache_key_for_tools(
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        &"a".repeat(64),
        &selectors,
    )
    .expect("other Mise binary pin");
    assert_ne!(
        key, other_mise_binary,
        "bootstrap digest partitions cache key"
    );
    let ubuntu_a = key
        .replace("${{ env.VELNOR_CACHE_IMAGE_OS }}", "ubuntu24.04")
        .replace("${{ env.VELNOR_CACHE_IMAGE_VERSION }}", "20260928.1");
    let ubuntu_b = ubuntu_a.replace("20260928.1", "20261002.3");
    assert_ne!(ubuntu_a, ubuntu_b, "image revisions partition the key");

    for partial in [
        key.replace("-${{ env.VELNOR_CACHE_IMAGE_OS }}", ""),
        key.replace("${{ env.VELNOR_CACHE_IMAGE_VERSION }}", "${{ env.CI }}"),
        key.replace("${{ runner.arch }}", "${{ secrets.PWNED }}"),
    ] {
        assert!(!is_tools_cache_key(&partial), "bad key accepted: {partial}");
    }

    let restore = tools_restore_step(&key).expect("restore");
    assert_eq!(
        restore.condition.as_deref(),
        Some(TOOLS_CACHE_RESTORE_CONDITION)
    );
    let payload = tools_cache_path_input();
    let StepKind::Action {
        uses: restore_uses,
        with: restore_with,
        ..
    } = restore.kind
    else {
        panic!("restore must be an action");
    };
    assert_eq!(restore_uses, TOOLS_RESTORE_USES);
    assert_eq!(restore_with.get("key"), Some(&key));
    assert_eq!(restore_with.get("path"), Some(&payload));
    assert!(!restore_with.contains_key("restore-keys"));

    let save = tools_save_step(&key).expect("save");
    let expected_save_condition = format!(
        "{} && {}",
        velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION,
        TOOLS_CACHE_SAVE_CONDITION
    );
    assert_eq!(
        save.condition.as_deref(),
        Some(expected_save_condition.as_str())
    );
    let StepKind::Action {
        uses: save_uses,
        with: save_with,
        ..
    } = save.kind
    else {
        panic!("save must be an action");
    };
    assert_eq!(save_uses, TOOLS_SAVE_USES);
    assert_eq!(save_with.get("key"), Some(&key));
    assert_eq!(save_with.get("path"), Some(&payload));
    assert!(!save_with.contains_key("restore-keys"));
    assert_eq!(
        TOOLS_CACHE_PATHS,
        [
            TOOLS_MISE_BOOTSTRAP_BINARY,
            TOOLS_MISE_DATA_DIR,
            "${{ runner.temp }}/velnor/rustup",
            "${{ runner.temp }}/velnor/cargo/bin",
            "${{ runner.temp }}/velnor/cargo/.crates.toml",
            "${{ runner.temp }}/velnor/cargo/.crates2.json",
        ],
        "full isolated tool state, Rust proxies, and install metadata"
    );
    let paths = TOOLS_CACHE_PATHS
        .iter()
        .map(|path| (*path).to_owned())
        .collect::<Vec<_>>();
    assert!(
        velnor_actions_workflow_renderer::steps::cache_action_step(
            false,
            TOOLS_SAVE_USES,
            "tools",
            &key,
            &[],
            &paths,
        )
        .is_err(),
        "raw generic action cannot bypass the typed trusted-save wrapper"
    );
}

#[test]
fn tool_and_source_archives_have_disjoint_paths() {
    let sha = "a".repeat(40);
    let source_action = format!("actions/cache/restore@{sha}");
    for path in TOOLS_CACHE_PATHS {
        assert!(
            velnor_actions_workflow_renderer::steps::cache_action_step(
                true,
                &source_action,
                "sources",
                "sources-key",
                &[],
                &[path.to_owned()],
            )
            .is_err(),
            "tool payload path leaked into source layer: {path}"
        );
    }
    for source in [
        "${{ runner.temp }}/velnor/cargo/registry/cache",
        "${{ runner.temp }}/velnor/cargo/registry/index",
        "${{ runner.temp }}/velnor/cargo/git/db",
    ] {
        assert!(
            velnor_actions_workflow_renderer::steps::cache_action_step(
                true,
                TOOLS_RESTORE_USES,
                "tools",
                &tools_cache_key_for_tools(
                    "x86_64-unknown-linux-gnu",
                    "2026.9.16",
                    MISE_SHA256,
                    &["rust@1.98.1".to_owned()],
                )
                .expect("key"),
                &[],
                &[source.to_owned()],
            )
            .is_err(),
            "source path leaked into tool layer: {source}"
        );
    }
}

fn run_identity(
    script: &str,
    env_file: &Path,
    image_os: Option<&str>,
    image_version: Option<&str>,
) -> String {
    fs::write(env_file, "").expect("reset env fixture");
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(script)
        .env("GITHUB_ENV", env_file)
        .env(TOOLS_CACHE_ELIGIBLE_ENV, "true");
    set_optional_env(&mut command, "ImageOS", image_os);
    set_optional_env(&mut command, "ImageVersion", image_version);
    let status = command.status().expect("run image helper");
    assert!(status.success(), "cold-path identity helper must succeed");
    fs::read_to_string(env_file).expect("read emitted GitHub env")
}

fn set_optional_env(command: &mut Command, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        command.env(name, value);
    } else {
        command.env_remove(name);
    }
}
