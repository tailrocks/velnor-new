//! T22 never-archive exclusions in rendered cache steps.
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::ambient_shell_step;
use velnor_actions_workflow_renderer::cache_p08::infer_job_tools;
use velnor_actions_workflow_renderer::steps::{
    NEVER_ARCHIVE_MARKERS, TOOLS_CACHE_PATHS, TOOLS_RESTORE_USES, cache_action_step,
    is_never_archive_path, workspace_cache_guard_step,
};
use velnor_actions_workflow_renderer::tofu_cache::{
    TOFU_PROVIDER_CACHE_BASE_EXPR, tofu_providers_save_step,
};

use super::impl_renderer_fixtures::*;

const HOME: &str = ".velnor/cache/cargo";
const KEY: &str = "velnor-v1-sources-trusted-compat-snapshot";
const PROVIDER_KEY: &str = "velnor-v1-tofu-providers-x86_64-unknown-linux-gnu-1.13.1-root-0123456789ab-${{hashFiles('.terraform.lock.hcl')}}";

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(label: &str) -> std::io::Result<Self> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let root =
            std::env::temp_dir().join(format!("velnor-{label}-{}-{nonce}", std::process::id()));
        fs::create_dir(&root)?;
        Ok(Self(root))
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!("failed to remove test root {}: {error}", self.0.display());
            }
        }
    }
}

#[test]
fn never_archive_mirror_lists_state_plans_and_credentials() {
    assert_eq!(
        NEVER_ARCHIVE_MARKERS,
        ["credentials", ".tfstate", ".tfplan"]
    );
    for marker in NEVER_ARCHIVE_MARKERS {
        assert!(is_never_archive_path(&format!("/cache/x{marker}")));
    }
    for clean in [
        "/cache/registry/cache/serde-1.0.228.crate",
        "/cache/.crates.toml",
        "/cache/bin/cargo-nextest",
    ] {
        assert!(!is_never_archive_path(clean), "{clean} stays archivable");
    }
}

#[test]
fn sources_steps_reject_never_archive_paths() -> Result<(), RenderError> {
    let good = format!("{HOME}/registry/cache");
    cache_action_step(true, TOOLS_RESTORE_USES, "sources", KEY, &[], &[good])?;
    for bad in [
        format!("{HOME}/registry/cache/state.tfstate"),
        format!("{HOME}/registry/cache/state.tfstate.backup"),
        format!("{HOME}/registry/cache/plan.tfplan"),
        format!("{HOME}/registry/cache/credentials.toml"),
        format!("{HOME}/.crates.toml"),
        format!("{HOME}/.crates2.json"),
        format!("{HOME}/bin"),
        format!("{HOME}/bin/cargo-nextest"),
        format!("{HOME}/bin/evil.tfplan"),
    ] {
        assert!(
            cache_action_step(
                true,
                TOOLS_RESTORE_USES,
                "sources",
                KEY,
                &[],
                std::slice::from_ref(&bad)
            )
            .is_err(),
            "must reject {bad}"
        );
    }
    Ok(())
}

fn infer_inline_tools(script: &str) -> Vec<String> {
    let step = ambient_shell_step(
        "Run inline tools",
        vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()],
        BTreeMap::new(),
    )
    .expect("valid inline shell step");
    let (_, job) = job("parser-test", "Parser test", Vec::new(), vec![step]);
    infer_job_tools(&job)
}

#[test]
fn inline_tool_inference_respects_shell_boundaries_and_quotes() {
    let both = ["gh@2.88.0".to_owned(), "rust@1.98.1".to_owned()];
    for script in [
        "mise exec rust@1.98.1 -- true; mise install gh@2.88.0",
        "mise exec rust@1.98.1 -- true&&mise install gh@2.88.0",
    ] {
        assert_eq!(infer_inline_tools(script), both, "script: {script}");
    }
    for script in [
        "mise exec rust@1.98.1 -- printf '%s' '&&' 'mise install gh@2.88.0'",
        "mise exec rust@1.98.1 -- printf '%s' '>&2' 'mise install gh@2.88.0'",
        "echo >&2 mise install gh@2.88.0",
        "echo 'x; mise install gh@2.88.0'",
    ] {
        assert_eq!(
            infer_inline_tools(script),
            if script.starts_with("mise exec") {
                vec!["rust@1.98.1".to_owned()]
            } else {
                Vec::new()
            },
            "operator and quoted text do not create command boundaries: {script}"
        );
    }
    assert_eq!(
        infer_inline_tools("mise exec rust@1.98.1 -- printf '%s' '&&' ; mise install gh@2.88.0"),
        both,
        "a real command separator after a quoted child argument is still recognized"
    );
}

#[test]
fn provider_steps_reject_never_archive_paths() -> Result<(), RenderError> {
    let good = format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/root-0123456789ab");
    tofu_providers_save_step(PROVIDER_KEY, &good)?;
    for bad in [
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/root-0123456789ab.tfstate"),
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/plan.tfplan"),
        format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/credentials-root-0123456789ab"),
    ] {
        assert!(
            tofu_providers_save_step(PROVIDER_KEY, &bad).is_err(),
            "must reject {bad}"
        );
    }
    Ok(())
}

#[test]
fn provider_save_carries_no_gate_itself() -> Result<(), RenderError> {
    let good = format!("{TOFU_PROVIDER_CACHE_BASE_EXPR}/root-0123456789ab");
    let save = tofu_providers_save_step(PROVIDER_KEY, &good)?;
    assert!(
        save.condition.is_none(),
        "the push gate arrives from the election caller, never the template"
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn workspace_cache_exclusion_is_idempotent_for_unterminated_git_exclude()
-> Result<(), Box<dyn std::error::Error>> {
    let step = workspace_cache_guard_step(true)?;
    let velnor_actions_contract::StepKind::Shell { run, .. } = &step.kind else {
        return Err("workspace cache exclusion must be a shell step".into());
    };
    let temp = TempRoot::new("cache-ignore")?;
    let workspace = temp.0.join("repo");
    fs::create_dir(&workspace)?;
    let initialized = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&workspace)
        .status()?;
    assert!(initialized.success(), "git init succeeds");
    let exclude = workspace.join(".git/info/exclude");
    fs::write(&exclude, b"# existing rule without final newline")?;
    let seed = workspace.join(".velnor/cache/seed");
    fs::create_dir_all(seed.parent().ok_or("cache seed parent")?)?;
    fs::write(&seed, b"private cache")?;
    let run_step = || -> Result<(), Box<dyn std::error::Error>> {
        let output = Command::new(&run[0])
            .args(&run[1..])
            .env("GITHUB_WORKSPACE", &workspace)
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other(String::from_utf8_lossy(&output.stderr)).into());
        }
        Ok(())
    };
    run_step()?;
    let first = fs::read(&exclude)?;
    run_step()?;
    let second = fs::read(&exclude)?;
    assert_eq!(
        first,
        b"# existing rule without final newline\n/.velnor/cache/\n"
    );
    assert_eq!(second, first, "rerunning the step adds no duplicate rule");
    Ok(())
}

#[test]
#[cfg(unix)]
fn workspace_cache_exclusion_rejects_symlinked_cache_before_touching_target()
-> Result<(), Box<dyn std::error::Error>> {
    let step = workspace_cache_guard_step(true)?;
    let velnor_actions_contract::StepKind::Shell { run, .. } = &step.kind else {
        return Err("workspace cache exclusion must be a shell step".into());
    };
    let temp = TempRoot::new("cache-ignore-symlink")?;
    let workspace = temp.0.join("repo");
    let outside = temp.0.join("outside");
    fs::create_dir(&workspace)?;
    fs::create_dir(&outside)?;
    let initialized = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(&workspace)
        .status()?;
    assert!(initialized.success(), "git init succeeds");
    let sentinel = outside.join("sentinel");
    fs::write(&sentinel, b"outside data stays untouched")?;
    fs::create_dir(workspace.join(".velnor"))?;
    std::os::unix::fs::symlink(&outside, workspace.join(".velnor/cache"))?;

    let output = Command::new(&run[0])
        .args(&run[1..])
        .env("GITHUB_WORKSPACE", &workspace)
        .output()?;
    assert!(
        !output.status.success(),
        "symlinked cache root must fail closed"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("velnor_cache_symlink:.velnor/cache"),
        "guard reports the symlinked component: {stderr}"
    );
    assert_eq!(fs::read(&sentinel)?, b"outside data stays untouched");
    assert_eq!(
        fs::read_dir(&outside)?.count(),
        1,
        "guard made no outside files"
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn tools_cache_archive_relocates_between_workspace_depths() -> Result<(), Box<dyn std::error::Error>>
{
    let expected = [
        ".velnor/cache/mise",
        ".velnor/cache/rustup",
        ".velnor/cache/cargo/bin",
    ];
    assert_eq!(TOOLS_CACHE_PATHS, expected);
    let temp = TempRoot::new("cache-relocation")?;
    let producer = temp.0.join("producer/nested/workspace");
    let consumer = temp.0.join("consumer/workflow/repository/checkout");
    let home = temp.0.join("unrelated-home");
    fs::create_dir_all(&producer)?;
    fs::create_dir_all(&consumer)?;
    fs::create_dir_all(&home)?;
    assert_ne!(
        producer.components().count(),
        consumer.components().count(),
        "consumer workspace is at a different path depth"
    );
    let payloads = [
        (
            TOOLS_CACHE_PATHS[0],
            "downloads/rust/1.98.1.tar",
            "mise install",
        ),
        (
            TOOLS_CACHE_PATHS[1],
            "toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/rustc",
            "rustup toolchain",
        ),
        (TOOLS_CACHE_PATHS[2], "rustc", "cargo proxy"),
    ];
    for (cache_path, suffix, contents) in payloads {
        let source = producer.join(cache_path).join(suffix);
        fs::create_dir_all(source.parent().ok_or("payload parent")?)?;
        fs::write(source, contents)?;
    }
    let cargo_bin = producer.join(TOOLS_CACHE_PATHS[2]);
    let mise_install = producer
        .join(TOOLS_CACHE_PATHS[0])
        .join("installs/rust/1.98.1");
    fs::create_dir_all(mise_install.parent().ok_or("Mise install parent")?)?;
    std::os::unix::fs::symlink(&cargo_bin, &mise_install)?;
    assert_eq!(fs::read_link(&mise_install)?, cargo_bin);
    let archive = temp.0.join("tools.tar");
    let mut create = Command::new("tar");
    create.arg("-cf").arg(&archive).arg("-C").arg(&producer);
    create.args(TOOLS_CACHE_PATHS);
    assert!(create.status()?.success(), "archive tool payloads");
    let listing = Command::new("tar").arg("-tf").arg(&archive).output()?;
    assert!(listing.status.success(), "list archive members");
    let listing = String::from_utf8(listing.stdout)?;
    for path in TOOLS_CACHE_PATHS {
        assert!(
            listing
                .lines()
                .any(|entry| entry.trim_end_matches('/') == path),
            "missing {path}"
        );
    }
    assert!(
        listing.lines().any(|entry| {
            entry.trim_end_matches('/') == ".velnor/cache/mise/installs/rust/1.98.1"
        }),
        "archive includes the Mise install symlink"
    );
    assert!(!listing.contains(producer.to_string_lossy().as_ref()));
    assert!(!listing.contains(home.to_string_lossy().as_ref()));
    fs::remove_dir_all(&producer)?;
    assert!(!producer.exists(), "producer root is gone before restore");
    let extracted = Command::new("tar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&consumer)
        .status()?;
    assert!(extracted.success(), "extract under consumer workspace");
    let restored_mise_install = consumer
        .join(TOOLS_CACHE_PATHS[0])
        .join("installs/rust/1.98.1");
    assert!(
        fs::symlink_metadata(&restored_mise_install)?
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::read_link(&restored_mise_install)?,
        cargo_bin,
        "archive preserves the producer-absolute tool link"
    );
    assert!(
        !restored_mise_install.exists(),
        "stale producer target stays absent at the deeper consumer workspace"
    );
    for (cache_path, suffix, contents) in payloads {
        let consumer_payload = consumer.join(cache_path).join(suffix);
        assert!(consumer_payload.starts_with(&consumer));
        assert_eq!(fs::read_to_string(consumer_payload)?, contents);
    }
    for old_home_path in [
        home.join(".local/share/mise/installs/rust/1.98.1/bin/rustc"),
        home.join(".rustup/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/rustc"),
        home.join(".cargo/bin/rustc"),
    ] {
        assert!(
            !old_home_path.exists(),
            "legacy path stayed absent: {}",
            old_home_path.display()
        );
    }
    for old_home_suffix in [
        ".local/share/mise/installs/rust/1.98.1/bin/rustc",
        ".rustup/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/rustc",
        ".cargo/bin/rustc",
    ] {
        assert!(
            !consumer.join(old_home_suffix).exists(),
            "payload must not be restored under HOME-like consumer path {}",
            consumer.join(old_home_suffix).display()
        );
    }
    Ok(())
}
