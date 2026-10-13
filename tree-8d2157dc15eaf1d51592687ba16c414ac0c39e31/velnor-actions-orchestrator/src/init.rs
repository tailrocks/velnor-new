//! `.velnor/config.toml` creation (missing files only, never overwrite).

use std::fs::OpenOptions;
use std::path::Path;

use velnor_actions_workflow_renderer::marker::marker_for_version;

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
# described in docs/implemented/named-mise-checks.md before adding checks.

# Velnor replaces the entire .github tree on generate. Keep CODEOWNERS at the
# repository root or under docs/ (both are GitHub-recognized); anything inside
# .github is removed.

# Optional workflow display and policy settings. Omitted values use Velnor defaults.
# [workflow]
# name = "CI"                         # Workflow display name.
# policy = "consumer-v1"              # Only consumer policy; Velnor's reserved policy works only in tailrocks/velnor-new.
# default_branch = "main"             # Push branch override; omit to use origin/HEAD. Required if origin/HEAD is unavailable.
# runner_label = "ubuntu-24.04" # Exact older pinned runner for compat; omit for the ubuntu-26.04 default.
# generator_validation = "bootstrap"  # Generator validation mode.
# max_parallel_jobs = 2                # Maximum generated matrix concurrency.

# Optional verification jobs, one support job per entry, all covered by the
# required gate. Unknown names fail generate. Enabled jobs may need
# repository-owned policy files (.alint.yml, .zizmor.yml).
# [workflow.verify]
# jobs = ["zizmor", "alint", "markdownlint", "strict-json", "frontmatter-id", "link-check", "native-validators"]

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

# Optional repository-relative POSIX globs excluded before detector input.
# [discovery]
# exclude = []

# Optional exact action-pin overrides. Omitted names use Velnor's bundled latest pins.
# [actions.overrides]
# "jdx/mise-action" = { version = "v5.1.1", sha = "2d8d4cafcbd33be2ea37d2b6f5ad595363d1f1ca" }
# "actions/checkout" = { version = "v7.0.1", sha = "3d3c42e5aac5ba805825da76410c181273ba90b1" }
# "actions/cache/restore" = { version = "v6.1.0", sha = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9" }
# "actions/cache/save" = { version = "v6.1.0", sha = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9" }
# "actions/upload-artifact" = { version = "v7.0.1", sha = "043fb46d1a93c77aae656e7c1c64a875d1fc6a0a" }
# "actions/download-artifact" = { version = "v8.0.1", sha = "3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c" }
# "jdx/mr-boxington-action" = { version = "v1.6.0", sha = "1687e54eb349cadf61fa38b5813a77875489e8e6" }
# Values must be an allowlisted action's matching release version and full SHA.
"#;
