//! `.velnor/config.toml` creation (missing files only, never overwrite).

use std::fs::OpenOptions;
use std::path::Path;

use velnor_actions_workflow_tree::marker::marker_for_version;

use crate::OrchestratorError;

/// Report for [`init_config`].
#[derive(Debug, Clone)]
pub struct InitReport {
    /// Repository-relative files created.
    pub created: Vec<String>,
}

/// Create the missing `.velnor/config.toml` sample, refusing overwrites.
///
/// Writes no other file. Fails when `.velnor` is an ordinary file, when
/// the config already exists, or when the root is unusable.
///
/// # Errors
///
/// Returns overwrite-refused, unsafe-path, render, or IO errors.
pub fn init_config(root: &Path) -> Result<InitReport, OrchestratorError> {
    let canonical = root
        .canonicalize()
        .map_err(|err| OrchestratorError::io(root.display().to_string(), err.to_string()))?;
    let dir = canonical.join(".velnor");
    match std::fs::symlink_metadata(&dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(OrchestratorError::UnsafePath {
                path: dir.display().to_string(),
                reason: "not_a_directory".to_owned(),
            });
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&dir)
                .map_err(|err| OrchestratorError::io(dir.display().to_string(), err.to_string()))?;
        }
        Err(err) => {
            return Err(OrchestratorError::io(
                dir.display().to_string(),
                err.to_string(),
            ));
        }
    }
    let path = dir.join("config.toml");
    let sample = sample_text()?;
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(file) => {
            use std::io::Write;
            let mut file = file;
            file.write_all(sample.as_bytes()).map_err(|err| {
                OrchestratorError::io(path.display().to_string(), err.to_string())
            })?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(OrchestratorError::OverwriteRefused {
                path: path.display().to_string(),
            });
        }
        Err(err) => {
            return Err(OrchestratorError::io(
                path.display().to_string(),
                err.to_string(),
            ));
        }
    }
    Ok(InitReport {
        created: vec![".velnor/config.toml".to_owned()],
    })
}

/// Sample text: versioned marker, required schema, commented sections.
fn sample_text() -> Result<String, OrchestratorError> {
    let marker = marker_for_version(env!("CARGO_PKG_VERSION"))?;
    Ok(format!("{marker}\n{SAMPLE_BODY}"))
}

/// Commented sample body from cli-contract section 4.
const SAMPLE_BODY: &str = r#"schema = 1

# Optional repository-owned named Mise checks, independent of language stacks.
# checks = []
# Configure task names, explicit platforms, tool pins, and scenario evidence as
# described in docs/content/docs/implemented/named-mise-checks.mdx before adding checks.

# Velnor replaces the entire .github tree on generate. Keep CODEOWNERS at the
# repository root or under docs/ (both are GitHub-recognized); anything inside
# .github is removed.

# Optional workflow display and policy settings. Omitted values use Velnor defaults.
# [workflow]
# name = "CI"                         # Workflow display name.
# policy = "consumer-v1"              # Only consumer policy; Velnor's reserved policy works only in tailrocks/velnor-new.
# default_branch = "<branch>"         # Push branch override; omit to use origin/HEAD. Required if origin/HEAD is unavailable.
# runner_label = "ubuntu-24.04" # Exact older pinned runner for compat; omit for the ubuntu-26.04 default.
# generator_validation = "bootstrap"  # Generator validation mode.
# max_parallel_jobs = 2                # Maximum generated matrix concurrency.

# Optional isolated verification tasks. Authors and reviewers keep task bodies
# free of Rust compilation; V1 does not inspect them. Each declaration gets a
# read-only contents token and joins Required on every CI trigger.
# [[workflow.tasks]]
# id = "native-format"
# kind = "verification"
# mise_task = "desktop-format-check"
# runner = "macos-arm64"               # Or "linux-x64".
# timeout_minutes = 10                  # Bounded 1..=360.

# Optional resource limits for generated jobs.
# [resources]
# compiler_process_budget = 2           # MBX/Cargo compiler process budget.
# test_process_budget = 2               # Test process budget per generated lane.

# Optional test partitioning. Keep one shard unless measurement justifies more.
# [test_sharding]
# default_shards = 1                    # Default number of test partitions.
# by_manifest = {}                      # Manifest path to shard-count overrides.

# Optional exact registered stack IDs to suppress after detection.
# [stacks]
# ignore = []                           # Example: ["rust"].

# Optional Rust task configuration. The Rust detector is automatic in V1.
# [stacks.rust]
# configurations = [{ name = "default", features = ["default"], target = "host" }]
# compile_driver = "cargo"         # Sticky override: "cargo" (default) or "mbx". Without it, a repo-local Mise Cargo wrapper selects MBX. Each key overrides its own axis only; conflicts with durable evidence fail closed.
# test_runner = "cargo_test"       # Sticky override: "cargo_test" (default) or "cargo_nextest". Without it, .config/nextest.toml selects Nextest ([profile.ci] when declared, else the documented default profile).
# Do not put free-form Mise commands in credential-bearing Rust jobs.

# Optional shared Rust policy lane (any policy). The lane materializes the
# pinned release, verifies its SHA-256, validates the Alint config, runs
# Alint with fail-on-warning, and uploads check-gaps diagnostics.
# [stacks.rust.policy]
# version = "0.1.3"                   # Pinned rust-repository-policy release.
# sha256 = "104c0d8b3a827875776358f941aa88f1c5837c1009305076af9380f4e3fcda25"  # SHA-256 of the release tarball asset.
# profile = "rust-strict-v1"          # Mandatory strict profile (only value).

# Optional docs lane (any policy). Frozen Bun install, MDX frontmatter lint,
# typecheck, build, absolute-link validation, and route smoke. Omitted
# values use the reference fumadocs layout below.
# [docs]
# app_dir = "docs"                    # Repo-relative Bun app directory.
# content_dir = "content/docs"        # App-relative MDX collection directory.
# base_path = "/docs"                 # Site base path serving the collection.
# output_dir = ".output/public"       # App-relative build output directory.
# smoke_routes = ["/", "/docs"]       # Sorted, duplicate-free smoke routes.

# Optional repository-relative POSIX globs excluded before detector input.
# [discovery]
# exclude = []

# Optional exact action-pin overrides. Omitted names use Velnor's bundled latest pins.
# [actions.overrides]
# "jdx/mise-action" = { version = "v5.0.0", sha = "9149ea85001c7435d5a66bb127d6a1b6227cb0a5" }
# "actions/checkout" = { version = "v7.0.1", sha = "3d3c42e5aac5ba805825da76410c181273ba90b1" }
# "actions/cache/restore" = { version = "v6.1.0", sha = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9" }
# "actions/cache/save" = { version = "v6.1.0", sha = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9" }
# "actions/upload-artifact" = { version = "v7.0.1", sha = "043fb46d1a93c77aae656e7c1c64a875d1fc6a0a" }
# "actions/download-artifact" = { version = "v8.0.1", sha = "3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c" }
# "jdx/mr-boxington-action" = { version = "v1.6.0", sha = "1687e54eb349cadf61fa38b5813a77875489e8e6" }
# "Swatinem/rust-cache" = { version = "v2.9.2", sha = "6323deb102c322ba6fcbdcafc7e3dddab59af2b6" }
# Values must be an allowlisted action's matching release version and full SHA.
"#;
