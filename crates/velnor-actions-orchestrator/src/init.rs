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
# default_branch = "main"             # Push branch override; omit to use origin/HEAD, then the CI event payload. Required if neither is available.
# runner_label = "ubuntu-24.04" # Exact older pinned runner for compat; omit for the ubuntu-26.04 default.
# generator_validation = "bootstrap"  # Generator validation mode.
# max_parallel_jobs = 2                # Maximum generated matrix concurrency.
# crate_timeout_minutes = 5            # timeout-minutes for every Rust crate job and OpenTofu root job; bounded 1..=360.

# Optional verification jobs, one support job per entry, all covered by the
# required gate. Unknown names fail generate. Enabled jobs may need
# repository-owned policy files (.alint.yml, .zizmor.yml).
# [workflow.verify]
# jobs = ["zizmor", "alint", "markdownlint", "strict-json", "frontmatter-id", "link-check", "native-validators"]
# strict_json_files = [".github/marketplace.json"] # Optional explicit scope; omit to check every JSON file.
# lychee_args = ["--offline", "--include-fragments"] # Optional Lychee args; preserved as separate argv values.

# Optional isolated verification and build tasks. V1 does not inspect task
# bodies for Rust compilation; task authors own that behavior. Each declaration
# gets a read-only contents token and joins Required on every CI trigger.
# [[workflow.tasks]]
# id = "native-desktop"
# kind = "build"                       # Hosted macOS 26 ARM64; joins Required.
# mise_task = "desktop-ci"
# source = { mise_config = "native/mise.toml", working_directory = "native" }
# tools = ["mr-boxington", "rust"]    # Selected closure comes from root mise.toml/mise.lock.
# runner = "macos-26-arm64"
# timeout_minutes = 5                 # Bounded 1..=360; default 5.
# cargo_build_jobs = 2                  # Bounded 1..=2; MBX/Rust are selected, while task code owns compiler selection.
# nextest_test_threads = 2              # Bounded 1..=2.
# [[workflow.tasks]]
# id = "native-format"
# kind = "verification"
# mise_task = "desktop-format-check"
# source = { mise_config = "native/mise.toml", working_directory = "native" }
# runner = "macos-arm64"               # Or "linux-x64".
# timeout_minutes = 5                  # Bounded 1..=360; default 5.

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
# compile_driver = "mbx"           # Sticky override: "cargo" (default) or "mbx". This sample's selected Rust toolchain enables MBX; V1 does not inspect task bodies to confirm their compiler route. Conflicts with durable evidence fail closed.
# test_runner = "cargo_test"       # Sticky override: "cargo_test" (default) or "cargo_nextest". Without it, .config/nextest.toml selects Nextest ([profile.ci] when declared, else the documented default profile).
# Do not put free-form Mise commands in credential-bearing Rust jobs.

# Optional per-package Cargo compile parallelism cap for memory-heavy crates.
# [stacks.rust.crates.repo-scan]
# build_jobs = 1                   # CARGO_BUILD_JOBS for every Cargo step of that crate job; 1..=64 and at most resources.compiler_process_budget. The package must exist.

# Optional generated .github/dco.yml for the cncf/dco2 GitHub App. Absent emits no file.
# [dco]
# allow_override_action = true      # allowOverrideAction: show the "Set DCO to pass" button.
# allow_remediation_commits = { individual = true, third_party = true }  # third_party requires individual.
# require = { members = false }     # members = false exempts organization members from sign-off.

# Optional shared native binary inventory (consumer-v1 only).
# Configure the producer and capture qualified_tools before enabling a channel.
# See docs/proposed/native-preview-contract.md and
# docs/proposed/binary-release-contract.md.
# [stacks.rust.binary_release]
# manifest_path = "Cargo.toml"      # Workspace-root manifest with a tracked Cargo.lock.
# version_package = "repo-scan"    # Exact product package owning the Cargo version.
# version_env = "REPO_SCAN_VERSION_OVERRIDE"
# [stacks.rust.binary_release.evidence]
# profile = "signed-sbom-v1"
# [[stacks.rust.binary_release.products]]
# id = "repo-scan"
# package = "repo-scan"
# bins = ["repo-scan"]             # Binary targets without required-features.
# targets = ["aarch64-apple-darwin"]
# archive = "tar.gz"
# [stacks.rust.binary_release.preview]
# enabled = false                 # Enable only after qualification and existing release admission.
# release_id = 0                  # Replace with the actual existing mutable preview release ID.
# [stacks.rust.binary_release.stable]
# enabled = false                 # Requires matching manual git-tag release-plz version policy.

# Optional repository-relative POSIX globs excluded before detector input.
# [discovery]
# exclude = []

# Optional exact action-pin overrides. Omitted names use Velnor's bundled latest pins.
# [actions.overrides]
# "jdx/mise-action" = { version = "v5.2.1", sha = "fc42bcc7ddbf51124c30f95a877a0c5168a3bfde" }
# "actions/checkout" = { version = "v7.0.1", sha = "3d3c42e5aac5ba805825da76410c181273ba90b1" }
# "actions/cache/restore" = { version = "v6.1.0", sha = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9" }
# "actions/cache/save" = { version = "v6.1.0", sha = "55cc8345863c7cc4c66a329aec7e433d2d1c52a9" }
# "actions/upload-artifact" = { version = "v7.0.2", sha = "cf430e030ddbb5b0abf93d22962f4752f3646cd9" }
# "actions/download-artifact" = { version = "v8.0.2", sha = "9000827ccba6bdab643e8b6fd33ac0654aef8333" }
# "jdx/mr-boxington-action" = { version = "v1.7.2", sha = "47ce49b32025d93a7a2f4570815afaa2b7092240" }
# Values must be an allowlisted action's matching release version and full SHA.
"#;
