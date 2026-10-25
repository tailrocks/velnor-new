use std::ffi::{OsStr, OsString};
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::path::{Path, PathBuf};

use super::{PinnedTool, ToolCatalog};
use crate::command::IsolatedCommand;
use crate::error::MiseError;
use crate::requests::PinnedToolExec;

pub const RELEASE_PLZ_CKSUM: &str =
    "be09b48beb7dc1fa436c95c21aae9df393e0cccbce7dd7c114dfbbf0afdd121f";

pub const REGISTRY_TOKEN_ENV: &str = "CARGO_REGISTRY_TOKEN";

pub const REQUIRED_ID_TOKEN_PERMISSION: &str = "write";

pub const TRUSTED_PUBLISHING_TOKENS_URL: &str =
    "https://crates.io/api/v1/trusted_publishing/tokens";

const RELEASE_PLZ_PROGRAM: &str = "release-plz";

const COORDINATOR_TOOLS: [PinnedTool; 2] = [PinnedTool::Rust, PinnedTool::ReleasePlz];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePrRequest {
    config: PathBuf,
    manifest: Option<PathBuf>,
    registry: Option<String>,
    package: Option<String>,
    json: bool,
}

impl ReleasePrRequest {
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

    pub fn with_manifest(mut self, manifest: PathBuf) -> Result<Self, MiseError> {
        if manifest.as_os_str().is_empty() {
            return Err(MiseError::InvalidManifestPath {
                path: String::new(),
            });
        }
        self.manifest = Some(manifest);
        Ok(self)
    }

    pub fn with_registry(mut self, registry: &str) -> Result<Self, MiseError> {
        reject_empty("registry", OsStr::new(registry))?;
        self.registry = Some(registry.to_owned());
        Ok(self)
    }

    pub fn with_package(mut self, package: &str) -> Result<Self, MiseError> {
        reject_empty("package", OsStr::new(package))?;
        self.package = Some(package.to_owned());
        Ok(self)
    }

    #[must_use]
    pub fn with_json_output(mut self) -> Self {
        self.json = true;
        self
    }

    #[must_use]
    pub fn release_pr_argv(&self) -> Vec<OsString> {
        let mut payload = vec![OsString::from(RELEASE_PLZ_PROGRAM)];
        payload.extend(self.args());
        payload
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        PinnedToolExec::new(
            COORDINATOR_TOOLS.to_vec(),
            OsStr::new(RELEASE_PLZ_PROGRAM),
            self.args(),
        )?
        .command(catalog)
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseAuth {
    Token,
    Oidc,
}

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
    pub fn release(config: PathBuf) -> Result<Self, MiseError> {
        reject_empty("config", config.as_os_str())?;
        Ok(Self::base(config))
    }

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

    pub fn dry_run(config: PathBuf) -> Result<Self, MiseError> {
        reject_empty("config", config.as_os_str())?;
        let mut request = Self::base(config);
        request.dry_run = true;
        Ok(request)
    }

    pub fn with_manifest(mut self, manifest: PathBuf) -> Result<Self, MiseError> {
        if manifest.as_os_str().is_empty() {
            return Err(MiseError::InvalidManifestPath {
                path: String::new(),
            });
        }
        self.manifest = Some(manifest);
        Ok(self)
    }

    pub fn with_registry(mut self, registry: &str) -> Result<Self, MiseError> {
        reject_empty("registry", OsStr::new(registry))?;
        self.registry = Some(registry.to_owned());
        Ok(self)
    }

    #[must_use]
    pub fn with_json_output(mut self) -> Self {
        self.json = true;
        self
    }

    #[must_use]
    pub fn release_argv(&self) -> Vec<OsString> {
        let mut payload = vec![OsString::from(RELEASE_PLZ_PROGRAM)];
        payload.extend(self.args());
        payload
    }

    pub fn command(&self, catalog: &ToolCatalog) -> Result<IsolatedCommand, MiseError> {
        PinnedToolExec::new(
            COORDINATOR_TOOLS.to_vec(),
            OsStr::new(RELEASE_PLZ_PROGRAM),
            self.args(),
        )?
        .command(catalog)
    }

    #[must_use]
    pub fn dry_run_active(&self) -> bool {
        self.dry_run
    }

    #[must_use]
    pub fn auth(&self) -> ReleaseAuth {
        self.auth
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OidcEnvironment {
    github_actions: bool,
    registry_token_env_present: bool,
}

impl OidcEnvironment {
    #[must_use]
    pub fn new(github_actions: bool, registry_token_env_present: bool) -> Self {
        Self {
            github_actions,
            registry_token_env_present,
        }
    }

    #[must_use]
    pub fn from_process_env() -> Self {
        Self {
            github_actions: std::env::var("GITHUB_ACTIONS").is_ok(),
            registry_token_env_present: std::env::var(REGISTRY_TOKEN_ENV).is_ok(),
        }
    }
}

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

fn reject_empty(field: &str, value: &OsStr) -> Result<(), MiseError> {
    if value.is_empty() {
        return Err(MiseError::InvalidStepInput {
            field: field.to_owned(),
            value: String::new(),
        });
    }
    Ok(())
}

fn push_path_flag(args: &mut Vec<OsString>, flag: &str, value: Option<&Path>) {
    if let Some(path) = value {
        args.extend([OsString::from(flag), path.as_os_str().to_owned()]);
    }
}

fn push_str_flag(args: &mut Vec<OsString>, flag: &str, value: Option<&str>) {
    if let Some(text) = value {
        args.extend([OsString::from(flag), OsString::from(text)]);
    }
}
