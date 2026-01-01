//! Pinned release-plz coordinator: fixed argv for the release-pr/release phases.
//!
//! Pin: CLI `0.3.169` (crates.io `max_stable_version` plus tag
//! `release-plz-v0.3.169`, published 2026-09-19; rechecked 2026-09-30).
//! Cksum: [`crate::catalog::release_plz::RELEASE_PLZ_CKSUM`]. Install: mise `release-plz` shorthand via
//! the aqua backend to the prebuilt GitHub tarball (isolated probe passed);
//! `cargo:release-plz` fallback compiles the cksum-pinned `.crate`. No
//! upstream per-asset SHA256 or sigstore exists, so pin plus cksum is the
//! integrity anchor. Reference only (never an allowed action):
//! `release-plz/action@v0.5.139` is `b8d6b54b02889ff2ae2bb82e8b57c3a8fc1683a5`.
//! [`crate::catalog::release_plz::ReleasePrRequest`] and [`crate::catalog::release_plz::ReleaseRequest`] never share one call;
//! `--config` is always explicit (upstream would silently fall back to
//! ambient files, then defaults); `release` has no `-p`, `release-pr -p`
//! takes one package, and `--no-verify`/`--allow-dirty` constructors do
//! not exist. `release --dry-run -o json` records nothing by design, so
//! previews parse logs. [`crate::catalog::release_plz::ReleaseAuth::Oidc`] renders zero token material;
//! the publish job then needs `id-token: write` and no [`crate::catalog::release_plz::REGISTRY_TOKEN_ENV`].

use std::ffi::{OsStr, OsString};
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::path::{Path, PathBuf};

use super::{PinnedTool, ToolCatalog};
use crate::command::IsolatedCommand;
use crate::error::MiseError;
use crate::requests::PinnedToolExec;

/// Full crates.io sha256 cksum of `release-plz 0.3.169` (API plus sparse index agree).
pub const RELEASE_PLZ_CKSUM: &str =
    "2f7a1b17465db464a28627bae7832ff7eb9b5f29b4fe89048b4dde8da1f567e5";

/// Registry token env var that must be absent for trusted publishing.
pub const REGISTRY_TOKEN_ENV: &str = "CARGO_REGISTRY_TOKEN";

/// Publish-job `id-token` permission the OIDC exchange requires.
pub const REQUIRED_ID_TOKEN_PERMISSION: &str = "write";

/// Crates.io endpoint exchanging the GitHub OIDC JWT for a publish token.
pub const TRUSTED_PUBLISHING_TOKENS_URL: &str =
    "https://crates.io/api/v1/trusted_publishing/tokens";

/// Payload program executed after the `--` separator.
const RELEASE_PLZ_PROGRAM: &str = "release-plz";

/// Coordinator tools: `release-plz` shells out to `cargo`, so pinned Rust rides along.
const COORDINATOR_TOOLS: [PinnedTool; 2] = [PinnedTool::Rust, PinnedTool::ReleasePlz];

/// Phase 1: `release-plz release-pr` with explicit `--config`; never publishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePrRequest {
    config: PathBuf,
    manifest: Option<PathBuf>,
    registry: Option<String>,
    package: Option<String>,
    json: bool,
}

impl ReleasePrRequest {
    /// Build the base `release-pr --config <PATH>` request.
    /// # Errors
    /// Returns [`MiseError::InvalidStepInput`] for an empty config path.
    pub fn new(config: PathBuf) -> Result<Self, MiseError> {
        reject_empty("config", config.as_os_str())?;
        Ok(Self {
            config,
            manifest: None,
            registry: None,
            package: None,
            json: false,
        })
    }

    /// Add a `--manifest-path` passthrough.
    /// # Errors
    /// Returns [`MiseError::InvalidManifestPath`] for an empty path.
    pub fn with_manifest(mut self, manifest: PathBuf) -> Result<Self, MiseError> {
        if manifest.as_os_str().is_empty() {
            return Err(MiseError::InvalidManifestPath {
                path: String::new(),
            });
        }
        self.manifest = Some(manifest);
        Ok(self)
    }

    /// Add a `--registry` passthrough.
    /// # Errors
    /// Returns [`MiseError::InvalidStepInput`] for an empty registry.
    pub fn with_registry(mut self, registry: &str) -> Result<Self, MiseError> {
        reject_empty("registry", OsStr::new(registry))?;
        self.registry = Some(registry.to_owned());
        Ok(self)
    }

    /// Target a single package with `-p`; upstream takes exactly one.
    /// # Errors
    /// Returns [`MiseError::InvalidStepInput`] for an empty package.
    pub fn with_package(mut self, package: &str) -> Result<Self, MiseError> {
        reject_empty("package", OsStr::new(package))?;
        self.package = Some(package.to_owned());
        Ok(self)
    }

    /// Render `-o json`: branch, URL, and number of the release PR.
    #[must_use]
    pub fn with_json_output(mut self) -> Self {
        self.json = true;
        self
    }

    /// Payload argv: program plus the fixed `release-pr` arguments.
    #[must_use]
    pub fn release_pr_argv(&self) -> Vec<OsString> {
        let mut payload = vec![OsString::from(RELEASE_PLZ_PROGRAM)];
        payload.extend(self.args());
        payload
    }

    /// Isolated command running this request.
    /// # Errors
    /// Returns [`MiseError::EmptyCommand`] for an empty payload (ruled out).
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        PinnedToolExec::new(
            COORDINATOR_TOOLS.to_vec(),
            OsStr::new(RELEASE_PLZ_PROGRAM),
            self.args(),
        )?
        .command(catalog)
    }

    /// Fixed `release-pr` arguments after the program.
    fn args(&self) -> Vec<OsString> {
        let mut args = vec![
            OsString::from("release-pr"),
            OsString::from("--config"),
            self.config.as_os_str().to_owned(),
        ];
        push_path_flag(&mut args, "--manifest-path", self.manifest.as_deref());
        push_str_flag(&mut args, "--registry", self.registry.as_deref());
        push_str_flag(&mut args, "-p", self.package.as_deref());
        if self.json {
            args.extend([OsString::from("-o"), OsString::from("json")]);
        }
        args
    }
}

/// Registry auth mode, fixed at construction: no silent fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseAuth {
    /// Explicit `--token <TOKEN>`; never inherited from the environment.
    Token,
    /// Zero token material; upstream may exchange GitHub OIDC instead.
    Oidc,
}

/// Phase 2: `release-plz release`; Debug never prints token material.
#[derive(Clone, PartialEq, Eq)]
pub struct ReleaseRequest {
    config: PathBuf,
    manifest: Option<PathBuf>,
    registry: Option<String>,
    token: Option<String>,
    dry_run: bool,
    json: bool,
    auth: ReleaseAuth,
}

#[expect(
    clippy::missing_fields_in_debug,
    reason = "token field redacted by design; Debug must never print it"
)]
impl Debug for ReleaseRequest {
    /// Debug without token material: only auth mode plus non-secret fields print.
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("ReleaseRequest")
            .field("config", &self.config)
            .field("manifest", &self.manifest)
            .field("registry", &self.registry)
            .field("dry_run", &self.dry_run)
            .field("json", &self.json)
            .field("auth", &self.auth)
            .finish()
    }
}

impl ReleaseRequest {
    /// Build a real `release --config <PATH>` request in OIDC mode.
    /// # Errors
    /// Returns [`MiseError::InvalidStepInput`] for an empty config path.
    pub fn release(config: PathBuf) -> Result<Self, MiseError> {
        reject_empty("config", config.as_os_str())?;
        Ok(Self::base(config))
    }

    /// Build a real `release` request with an explicit `--token` (argv holds it; secret).
    /// # Errors
    /// Returns [`MiseError::InvalidStepInput`] for an empty config path or token.
    pub fn release_with_token(config: PathBuf, token: &str) -> Result<Self, MiseError> {
        if token.is_empty() {
            return Err(MiseError::InvalidStepInput {
                field: "token".to_owned(),
                value: "empty_registry_token".to_owned(),
            });
        }
        reject_empty("config", config.as_os_str())?;
        let mut request = Self::base(config);
        request.token = Some(token.to_owned());
        request.auth = ReleaseAuth::Token;
        Ok(request)
    }

    /// Build a tokenless `release --dry-run` request: checks without uploading.
    /// # Errors
    /// Returns [`MiseError::InvalidStepInput`] for an empty config path.
    pub fn dry_run(config: PathBuf) -> Result<Self, MiseError> {
        reject_empty("config", config.as_os_str())?;
        let mut request = Self::base(config);
        request.dry_run = true;
        Ok(request)
    }

    /// Add a `--manifest-path` passthrough.
    /// # Errors
    /// Returns [`MiseError::InvalidManifestPath`] for an empty path.
    pub fn with_manifest(mut self, manifest: PathBuf) -> Result<Self, MiseError> {
        if manifest.as_os_str().is_empty() {
            return Err(MiseError::InvalidManifestPath {
                path: String::new(),
            });
        }
        self.manifest = Some(manifest);
        Ok(self)
    }

    /// Add a `--registry` passthrough (OIDC engages only for crates-io).
    /// # Errors
    /// Returns [`MiseError::InvalidStepInput`] for an empty registry.
    pub fn with_registry(mut self, registry: &str) -> Result<Self, MiseError> {
        reject_empty("registry", OsStr::new(registry))?;
        self.registry = Some(registry.to_owned());
        Ok(self)
    }

    /// Render `-o json`: version and tag of the released packages.
    #[must_use]
    pub fn with_json_output(mut self) -> Self {
        self.json = true;
        self
    }

    /// Payload argv: program plus the fixed `release` arguments.
    #[must_use]
    pub fn release_argv(&self) -> Vec<OsString> {
        let mut payload = vec![OsString::from(RELEASE_PLZ_PROGRAM)];
        payload.extend(self.args());
        payload
    }

    /// Isolated command running this request.
    /// # Errors
    /// Returns [`MiseError::EmptyCommand`] for an empty payload (ruled out).
    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        PinnedToolExec::new(
            COORDINATOR_TOOLS.to_vec(),
            OsStr::new(RELEASE_PLZ_PROGRAM),
            self.args(),
        )?
        .command(catalog)
    }

    /// Whether `--dry-run` is rendered.
    #[must_use]
    pub fn dry_run_active(&self) -> bool {
        self.dry_run
    }

    /// Auth mode fixed by the constructor.
    #[must_use]
    pub fn auth(&self) -> ReleaseAuth {
        self.auth
    }

    /// Base OIDC request; constructors set token or dry-run after this.
    fn base(config: PathBuf) -> Self {
        Self {
            config,
            manifest: None,
            registry: None,
            token: None,
            dry_run: false,
            json: false,
            auth: ReleaseAuth::Oidc,
        }
    }

    /// Fixed `release` arguments after the program.
    fn args(&self) -> Vec<OsString> {
        let mut args = vec![
            OsString::from("release"),
            OsString::from("--config"),
            self.config.as_os_str().to_owned(),
        ];
        push_path_flag(&mut args, "--manifest-path", self.manifest.as_deref());
        push_str_flag(&mut args, "--registry", self.registry.as_deref());
        push_str_flag(&mut args, "--token", self.token.as_deref());
        if self.dry_run {
            args.push(OsString::from("--dry-run"));
        }
        if self.json {
            args.extend([OsString::from("-o"), OsString::from("json")]);
        }
        args
    }
}

/// Observed process conditions for trusted-publishing auto-engagement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OidcEnvironment {
    github_actions: bool,
    registry_token_env_present: bool,
}

impl OidcEnvironment {
    /// Observe exact conditions (deterministic; prefer in tests).
    #[must_use]
    pub fn new(github_actions: bool, registry_token_env_present: bool) -> Self {
        Self {
            github_actions,
            registry_token_env_present,
        }
    }

    /// Read the live process: `GITHUB_ACTIONS` plus [`REGISTRY_TOKEN_ENV`] presence.
    #[must_use]
    pub fn from_process_env() -> Self {
        Self {
            github_actions: std::env::var("GITHUB_ACTIONS").is_ok(),
            registry_token_env_present: std::env::var(REGISTRY_TOKEN_ENV).is_ok(),
        }
    }
}

/// Whether upstream auto-engages trusted publishing (OIDC) for a request.
///
/// Mirrors the upstream gate: no flag token, no env token, crates-io
/// (unset or `crates-io`), real publish, no dry run, GitHub Actions.
/// Upstream mints at [`TRUSTED_PUBLISHING_TOKENS_URL`], revokes after, and
/// on exchange failure warns and proceeds without it, never erroring.
#[must_use]
pub fn trusted_publishing_engages(
    auth: ReleaseAuth,
    registry: Option<&str>,
    dry_run: bool,
    env: OidcEnvironment,
) -> bool {
    let tokenless = auth == ReleaseAuth::Oidc && !env.registry_token_env_present;
    let crates_io = registry.is_none_or(|name| name == "crates-io");
    tokenless && crates_io && !dry_run && env.github_actions
}

/// Reject an empty option value with its field name.
fn reject_empty(field: &str, value: &OsStr) -> Result<(), MiseError> {
    if value.is_empty() {
        return Err(MiseError::InvalidStepInput {
            field: field.to_owned(),
            value: String::new(),
        });
    }
    Ok(())
}

/// Append `--flag <path>` when the option is set.
fn push_path_flag(args: &mut Vec<OsString>, flag: &str, value: Option<&Path>) {
    if let Some(path) = value {
        args.extend([OsString::from(flag), path.as_os_str().to_owned()]);
    }
}

/// Append `--flag <value>` when the option is set.
fn push_str_flag(args: &mut Vec<OsString>, flag: &str, value: Option<&str>) {
    if let Some(text) = value {
        args.extend([OsString::from(flag), OsString::from(text)]);
    }
}
