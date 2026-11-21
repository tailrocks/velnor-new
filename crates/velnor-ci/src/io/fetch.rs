// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::FetchArgs;
use crate::error::CiError;
use crate::github::GitHubWrite;
use crate::trusted_roots::BUNDLE_VARIABLE;

pub(super) const TOKEN_VARIABLE: &str = "VELNOR_PRIVATE_TOKEN";
const API_VARIABLE: &str = "VELNOR_API_URL";
const SYSTEM_DIRECTORIES: &[&str] = &["/usr/bin", "/bin", "/usr/local/bin", "/opt/homebrew/bin"];
const FORWARDED_VARIABLES: &[&str] = &[
    BUNDLE_VARIABLE,
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
    "NO_PROXY",
    "no_proxy",
];
const GUARDED_GIT_SETTINGS: &[(&str, &str)] = &[
    ("core.hooksPath", "/dev/null"),
    ("core.fsmonitor", "false"),
    ("core.sshCommand", "ssh"),
    ("protocol.ext.allow", "never"),
    ("credential.helper", ""),
];
const COMPILER_SELECTORS: &[(&str, &str)] = &[
    ("RUSTC", "rustc"),
    ("CARGO_BUILD_RUSTC", "rustc"),
    ("RUSTC_WRAPPER", ""),
    ("RUSTC_WORKSPACE_WRAPPER", ""),
    ("CARGO_BUILD_RUSTC_WRAPPER", ""),
    ("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER", ""),
];
const TOKEN_GIT_PROTOCOLS: &str = "https";
const SERVER_VARIABLE: &str = "VELNOR_SERVER_URL";
const DEFAULT_SERVER: &str = "https://github.com";
pub(super) const REVOKE_PATH: &str = "/installation/token";
pub(super) const FORK_HINT: &str = "no token was created: a pull request from a fork gets none, and the sources cache did not hold every dependency";

pub(super) fn fetch(args: &FetchArgs) -> Result<String, CiError> {
    let token = std::env::var(TOKEN_VARIABLE)
        .ok()
        .filter(|token| !token.is_empty());
    let client = token
        .clone()
        .map(|token| api_client(token, super::http::DEFAULT_TIMEOUT));
    let outcome = fetch_with(args, token.as_deref());
    let revocation = client.map_or(Ok(()), |client| {
        client
            .and_then(|github| github.delete(REVOKE_PATH))
            .map_err(|e| {
                e.context("the fetch token could not be revoked; it expires within the hour")
            })
    });
    outcome?;
    revocation?;
    Ok(if token.is_some() {
        "Fetched the dependencies and revoked the token\n".to_owned()
    } else {
        "Fetched the dependencies from the restored caches without a token\n".to_owned()
    })
}

fn fetch_with(args: &FetchArgs, token: Option<&str>) -> Result<(), CiError> {
    let home = super::fetch_fixed::account_home(args.home.as_deref())?;
    let fixed = super::fetch_fixed::Fixed::system().find();
    let mut roots = args.trusted_dir.clone();
    roots.extend(fixed.iter().flat_map(super::fetch_fixed::Located::roots));
    let mut trusted = Trusted::new(&home, &roots);
    let mut inherited = Inherited {
        cargo_home: std::env::var_os("CARGO_HOME"),
        rustup_home: std::env::var_os("RUSTUP_HOME"),
    };
    let scratch = if fixed.is_some() || token.is_some() {
        let cache = match fixed {
            Some(_) => super::fetch_home::cache_home(&home, inherited.cargo_home.as_deref()),
            None => trusted.home_variable(
                inherited.cargo_home.as_ref(),
                home.join(".cargo"),
                "CARGO_HOME",
            )?,
        };
        if token.is_some() {
            super::fetch_git::check_databases(&cache)?;
        }
        let temp = super::fetch_home::temp_parent(&home);
        Some(super::fetch_home::Scratch::create(
            &cache,
            &temp,
            token.is_none(),
        )?)
    } else {
        None
    };
    if let Some(scratch) = &scratch {
        trusted.roots.push(scratch.path().to_path_buf());
        inherited = match &fixed {
            Some(fixed) => fixed.adopt(scratch.path()),
            None => Inherited {
                cargo_home: Some(scratch.path().into()),
                rustup_home: inherited.rustup_home,
            },
        };
    }
    let search = search_path();
    let mut cargo = super::fetch_fixed::cargo(fixed.as_ref(), &trusted, &home, &search)?;
    let git = trusted.program("git", &search, true)?;
    let mirror = super::crates_mirror::runner_config()?;
    let server = Server::from_environment()?;
    let neutral = scratch
        .as_ref()
        .filter(|_| token.is_some())
        .map(super::fetch_home::Scratch::path);
    let mut toolchain = None;
    if let Some(neutral) = neutral {
        let rustup_home = trusted.home_variable(
            inherited.rustup_home.as_ref(),
            home.join(".rustup"),
            "RUSTUP_HOME",
        )?;
        let resolved =
            super::fetch_toolchain::resolve(&cargo, &home, &rustup_home, neutral, &trusted)?;
        cargo = resolved.cargo;
        toolchain = Some(resolved.toolchain);
    }
    let mut environment = trusted.environment(&home, &cargo, &git, &inherited, token, &server)?;
    environment.extend(toolchain.map(|name| ("RUSTUP_TOOLCHAIN".into(), name)));
    super::fetch_run::run_fetches(
        &cargo,
        &environment,
        mirror.as_deref(),
        &args.manifest_path,
        neutral,
    )?;
    match (&scratch, token) {
        (Some(scratch), Some(_)) => scratch.keep_git(),
        _ => Ok(()),
    }
}

pub(super) fn api_client(
    token: String,
    timeout: Duration,
) -> Result<super::http::HttpGitHub, CiError> {
    let api = std::env::var(API_VARIABLE)
        .ok()
        .filter(|api| super::http::trusted_api(api))
        .ok_or_else(|| CiError::new(format!("set {API_VARIABLE} to github.api_url")))?;
    Ok(super::http::HttpGitHub::new(api, token, timeout))
}

pub(super) struct Server {
    authority: String,
    forms: Vec<String>,
}

impl Server {
    pub(super) fn from_environment() -> Result<Self, CiError> {
        let base = std::env::var(SERVER_VARIABLE).unwrap_or_else(|_| DEFAULT_SERVER.to_owned());
        let authority = base
            .strip_prefix("https://")
            .filter(|authority| plain_authority(authority))
            .ok_or_else(|| {
                CiError::new(format!(
                    "{SERVER_VARIABLE} must be https://<host>[:<port>] (github.server_url)"
                ))
            })?;
        let mut forms = vec![
            format!("https://{authority}/"),
            format!("http://{authority}/"),
        ];
        let host = authority.split(':').next().unwrap_or(authority);
        forms.extend([
            format!("ssh://git@{host}/"),
            format!("ssh://{host}/"),
            format!("git@{host}:"),
        ]);
        Ok(Self {
            authority: authority.to_owned(),
            forms,
        })
    }
}

fn plain_authority(authority: &str) -> bool {
    let (host, port) = authority
        .split_once(':')
        .map_or((authority, None), |(host, port)| (host, Some(port)));
    !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && port.is_none_or(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
}

pub(super) struct Inherited {
    pub(super) cargo_home: Option<OsString>,
    pub(super) rustup_home: Option<OsString>,
}

pub(super) struct Trusted {
    pub(super) roots: Vec<PathBuf>,
}

impl Trusted {
    pub(super) fn new(home: &Path, extra: &[PathBuf]) -> Self {
        let mut roots = vec![
            home.join(".local/share/mise"),
            home.join(".cargo"),
            home.join(".rustup"),
        ];
        roots.extend(extra.iter().cloned());
        Self {
            roots: roots
                .into_iter()
                .map(|root| std::fs::canonicalize(&root).unwrap_or(root))
                .collect(),
        }
    }

    pub(super) fn within(&self, path: &Path) -> bool {
        self.roots.iter().any(|root| path.starts_with(root))
    }

    pub(super) fn program(
        &self,
        name: &str,
        search: &[PathBuf],
        system: bool,
    ) -> Result<PathBuf, CiError> {
        let system_directories = SYSTEM_DIRECTORIES.iter().map(PathBuf::from);
        let directories = search
            .iter()
            .cloned()
            .chain(system.then_some(system_directories).into_iter().flatten());
        for directory in directories {
            let Ok(directory) = std::fs::canonicalize(&directory) else {
                continue;
            };
            let candidate = directory.join(name);
            let Ok(resolved) = std::fs::canonicalize(&candidate) else {
                continue;
            };
            let trusted_directory = self.within(&directory)
                || (system && SYSTEM_DIRECTORIES.iter().any(|d| directory == Path::new(d)));
            let resolved_trusted = !self.within(&directory) || self.within(&resolved);
            if trusted_directory
                && resolved_trusted
                && resolved.is_file()
                && !super::fetch_real_cargo::is_mise_front(&candidate, &resolved)
            {
                return Ok(candidate);
            }
        }
        Err(CiError::new(format!(
            "no real {name} in a tool directory the runner installed; the checkout cannot supply it, and a mise shim or wrapper is never run with the token"
        )))
    }

    pub(super) fn home_variable(
        &self,
        value: Option<&OsString>,
        default: PathBuf,
        name: &str,
    ) -> Result<PathBuf, CiError> {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return Ok(default);
        };
        let path = PathBuf::from(value);
        if path.is_absolute() && self.within(&path) {
            Ok(path)
        } else {
            Err(CiError::new(format!(
                "{name} is {}, outside the tool directories the runner installed",
                path.display()
            )))
        }
    }

    pub(super) fn environment(
        &self,
        home: &Path,
        cargo: &Path,
        git: &Path,
        inherited: &Inherited,
        token: Option<&str>,
        server: &Server,
    ) -> Result<Vec<(OsString, OsString)>, CiError> {
        let cargo_home = self.home_variable(
            inherited.cargo_home.as_ref(),
            home.join(".cargo"),
            "CARGO_HOME",
        )?;
        let rustup_home = self.home_variable(
            inherited.rustup_home.as_ref(),
            home.join(".rustup"),
            "RUSTUP_HOME",
        )?;
        let directories: Vec<PathBuf> = [cargo.parent(), git.parent()]
            .into_iter()
            .flatten()
            .map(Path::to_path_buf)
            .chain(["/usr/bin", "/bin"].map(PathBuf::from))
            .collect();
        let path = std::env::join_paths(directories)
            .map_err(|e| CiError::new(format!("cannot build PATH: {e}")))?;
        let mut environment: Vec<(OsString, OsString)> = vec![
            ("PATH".into(), path),
            ("HOME".into(), home.into()),
            ("CARGO_HOME".into(), cargo_home.into()),
            ("RUSTUP_HOME".into(), rustup_home.into()),
            ("RUSTUP_AUTO_INSTALL".into(), "0".into()),
            ("CARGO_NET_GIT_FETCH_WITH_CLI".into(), "true".into()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
            ("GIT_TERMINAL_PROMPT".into(), "0".into()),
        ];
        environment.extend(
            FORWARDED_VARIABLES
                .iter()
                .filter_map(|name| Some((OsString::from(name), non_empty_variable(name)?))),
        );
        if let Some(token) = token {
            environment.extend(
                COMPILER_SELECTORS
                    .iter()
                    .map(|(name, value)| (OsString::from(name), OsString::from(value))),
            );
            environment.push(("GIT_ALLOW_PROTOCOL".into(), TOKEN_GIT_PROTOCOLS.into()));
            let base = format!(
                "url.https://x-access-token:{token}@{}/.insteadOf",
                server.authority
            );
            let mut settings: Vec<(String, String)> = server
                .forms
                .iter()
                .map(|form| (base.clone(), form.clone()))
                .collect();
            settings.extend(
                GUARDED_GIT_SETTINGS
                    .iter()
                    .map(|(key, value)| ((*key).to_owned(), (*value).to_owned())),
            );
            environment.push(("GIT_CONFIG_COUNT".into(), settings.len().to_string().into()));
            for (index, (key, value)) in settings.into_iter().enumerate() {
                environment.push((format!("GIT_CONFIG_KEY_{index}").into(), key.into()));
                environment.push((format!("GIT_CONFIG_VALUE_{index}").into(), value.into()));
            }
        }
        Ok(environment)
    }
}

fn non_empty_variable(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

pub(super) fn search_path() -> Vec<PathBuf> {
    std::env::var_os("PATH").map_or_default(|path| std::env::split_paths(&path).collect())
}
