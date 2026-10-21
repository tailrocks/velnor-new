//! Pinned lint-tool versions and the jactionlint argument vectors.
//!
//! jactionlint and `ShellCheck` versions are pinned constants; Zizmor's pin
//! is explicit caller input validated for exactness. Types only: Mise
//! executes tools; this crate never spawns processes.

use crate::{ActionlintError, JACTIONLINT_VERSION};

/// Pinned `ShellCheck` release (tag `v0.11.0`; mirrors the mise catalog pin).
/// Source: `https://api.github.com/repos/koalaman/shellcheck/releases/latest`; checked 2026-09-28.
pub const SHELLCHECK_VERSION: &str = "0.11.0";

/// Generated jactionlint config, read through `--config-file` everywhere.
pub const JACTIONLINT_CONFIG_PATH: &str = ".github/jactionlint.yaml";

/// The file set every jactionlint run checks, as `hk`'s `Builtins.jactionlint`
/// globs it: workflows, composite actions and Dependabot. Nothing outside
/// these globs (for example golden trees under `docs/`) is ever linted.
pub const LINT_FILE_GLOBS: [&str; 3] = [
    ".github/workflows/*.y*ml",
    ".github/actions/**/action.y*ml",
    ".github/dependabot.y*ml",
];

/// Flags every Velnor jactionlint run passes, in CI, release gates, staged
/// validation and the documented `hk` step.
///
/// - `--strict-exit`: any finding fails, whatever its level.
/// - `--no-online`: no network check, even if a pull request sets
///   `online: true` in the config (the flag wins over the file).
/// - `--shellcheck=shellcheck`: the pinned `ShellCheck` on `PATH`; a missing
///   binary is exit 3, never a silent pass.
/// - `--pyflakes=`: no other `PATH` binary runs.
/// - `--no-color`: logs and annotations stay plain.
pub const LINT_FLAGS: [&str; 5] = [
    "--strict-exit",
    "--no-online",
    "--shellcheck=shellcheck",
    "--pyflakes=",
    "--no-color",
];

/// Pinned jactionlint toolchain identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JactionlintToolchain {
    /// Exact pinned version (always [`JACTIONLINT_VERSION`]).
    version: String,
}

impl JactionlintToolchain {
    /// Toolchain at the pinned jactionlint release.
    #[must_use]
    pub fn pinned() -> Self {
        Self {
            version: JACTIONLINT_VERSION.to_owned(),
        }
    }

    /// Exact pinned version string.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Mise tool spec (`jactionlint@<exact>`).
    #[must_use]
    pub fn mise_tool_spec(&self) -> String {
        format!("jactionlint@{}", self.version)
    }

    /// Staged lint arguments for one pinned-binary invocation.
    ///
    /// Returns `[--config-file, config, --format, oneline, ...LINT_FLAGS,
    /// ...files]`: the staged config plus every staged workflow and composite
    /// action in one invocation. Pure data: the caller spawns via Mise.
    #[must_use]
    pub fn staged_lint_argv(config_path: &str, files: &[String]) -> Vec<String> {
        let mut argv = vec![
            "--config-file".to_owned(),
            config_path.to_owned(),
            "--format".to_owned(),
            "oneline".to_owned(),
        ];
        argv.extend(LINT_FLAGS.iter().map(|flag| (*flag).to_owned()));
        argv.extend(files.iter().cloned());
        argv
    }
}

impl Default for JactionlintToolchain {
    fn default() -> Self {
        Self::pinned()
    }
}

/// `find` arguments enumerating [`LINT_FILE_GLOBS`] portably: the same set
/// as the globs, without needing bash 4 `globstar` (macOS ships bash 3.2).
const LINT_FILE_FINDS: [&str; 3] = [
    ".github/workflows -maxdepth 1 -type f -name \"*.y*ml\"",
    ".github/actions -type f -name \"action.y*ml\"",
    ".github -maxdepth 1 -type f -name \"dependabot.y*ml\"",
];

/// Inline script for every generated repository-wide jactionlint step.
///
/// The CI `jactionlint` job and the release eligibility gates lint the
/// committed tree, so they share this one script: it lists the files
/// [`LINT_FILE_GLOBS`] match (sorted, byte order), fails closed when there
/// are none (a bare run would scan the whole repository, golden trees
/// included), then `exec`s `jactionlint_command` (the caller's pinned
/// `mise ... exec ... -- jactionlint` prefix, plain words only) with GitHub
/// annotations, [`LINT_FLAGS`] and the files. Run it as `bash -c <script>`.
#[must_use]
pub fn workflow_lint_script(jactionlint_command: &str) -> String {
    let finds = LINT_FILE_FINDS
        .iter()
        .map(|args| format!("find {args}; "))
        .collect::<String>();
    format!(
        "files=(); while IFS= read -r file; do files+=(\"$file\"); done < <({{ {finds}}} 2>/dev/null | LC_ALL=C sort); if [ \"${{#files[@]}}\" -eq 0 ]; then echo \"jactionlint: no workflow files matched\" >&2; exit 3; fi; exec {jactionlint_command} --config-file {JACTIONLINT_CONFIG_PATH} --format github {flags} \"${{files[@]}}\"",
        flags = LINT_FLAGS.join(" "),
    )
}

/// Pinned `ShellCheck` toolchain identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellcheckToolchain {
    /// Exact pinned version (always [`SHELLCHECK_VERSION`]).
    version: String,
}

impl ShellcheckToolchain {
    /// Toolchain at the pinned `ShellCheck` release.
    #[must_use]
    pub fn pinned() -> Self {
        Self {
            version: SHELLCHECK_VERSION.to_owned(),
        }
    }

    /// Exact pinned version string.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Mise tool spec (`shellcheck@<exact>`).
    #[must_use]
    pub fn mise_tool_spec(&self) -> String {
        format!("shellcheck@{}", self.version)
    }
}

impl Default for ShellcheckToolchain {
    fn default() -> Self {
        Self::pinned()
    }
}

/// Zizmor toolchain identity with an explicit exact pin.
///
/// No default: the spec pins no Zizmor version, so callers must supply
/// the reviewed exact release; anything else fails validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZizmorToolchain {
    /// Exact reviewed Zizmor release (`X.Y.Z`, no prefix).
    version: String,
}

impl ZizmorToolchain {
    /// Build a Zizmor toolchain from an explicit exact version.
    ///
    /// # Errors
    ///
    /// Returns [`ActionlintError::InvalidToolVersion`] unless the version
    /// is exact `X.Y.Z` numeric with no prefix or prerelease.
    pub fn new(version: impl Into<String>) -> Result<Self, ActionlintError> {
        let version = version.into();
        if !is_exact_version(&version) {
            return Err(ActionlintError::InvalidToolVersion {
                tool: "zizmor",
                version,
            });
        }
        Ok(Self { version })
    }

    /// Exact pinned version string.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Mise tool spec (`zizmor@<exact>`).
    #[must_use]
    pub fn mise_tool_spec(&self) -> String {
        format!("zizmor@{}", self.version)
    }
}

/// Minimal typed config for the workflow-lint tool set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowLintTools {
    /// Pinned jactionlint toolchain.
    pub jactionlint: JactionlintToolchain,
    /// Pinned `ShellCheck` toolchain.
    pub shellcheck: ShellcheckToolchain,
    /// Explicitly pinned Zizmor toolchain.
    pub zizmor: ZizmorToolchain,
}

impl WorkflowLintTools {
    /// Build the lint-tool set from an explicit Zizmor version.
    ///
    /// # Errors
    ///
    /// Returns [`ActionlintError::InvalidToolVersion`] for a non-exact
    /// Zizmor version.
    pub fn new(zizmor_version: impl Into<String>) -> Result<Self, ActionlintError> {
        Ok(Self {
            jactionlint: JactionlintToolchain::pinned(),
            shellcheck: ShellcheckToolchain::pinned(),
            zizmor: ZizmorToolchain::new(zizmor_version)?,
        })
    }

    /// Mise tool specs in fixed order: jactionlint, shellcheck, zizmor.
    #[must_use]
    pub fn mise_tool_specs(&self) -> Vec<String> {
        vec![
            self.jactionlint.mise_tool_spec(),
            self.shellcheck.mise_tool_spec(),
            self.zizmor.mise_tool_spec(),
        ]
    }
}

/// Exact versions: `X.Y.Z` numeric, no `v` prefix, no prerelease.
fn is_exact_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}
