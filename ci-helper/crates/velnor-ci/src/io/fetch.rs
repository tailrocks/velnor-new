// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::FetchArgs;
use crate::error::CiError;
use crate::github::GitHubWrite;

const TOKEN_VARIABLE: &str = "VELNOR_PRIVATE_TOKEN";
const API_VARIABLE: &str = "VELNOR_API_URL";
const SYSTEM_DIRECTORIES: [&str; 4] = ["/usr/bin", "/bin", "/usr/local/bin", "/opt/homebrew/bin"];
const REVOKE_PATH: &str = "/installation/token";
const FORK_HINT: &str = "no token was created: a pull request from a fork gets none, and the sources cache did not hold every dependency";

pub(super) fn fetch(args: &FetchArgs) -> Result<String, CiError> {
    let token = std::env::var(TOKEN_VARIABLE)
        .ok()
        .filter(|token| !token.is_empty());
    let client = token.clone().map(api_client).transpose()?;
    let outcome = fetch_with(args, token.as_deref());
    let revocation = client.map_or(Ok(()), |github| {
        github.delete(REVOKE_PATH).map_err(|e| {
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
    let home = account_home()?;
    let trusted = Trusted::new(&home, &args.trusted_dir);
    let search = search_path();
    let cargo = trusted.program("cargo", &search, false)?;
    let git = trusted.program("git", &search, true)?;
    let inherited = Inherited {
        cargo_home: std::env::var_os("CARGO_HOME"),
        rustup_home: std::env::var_os("RUSTUP_HOME"),
    };
    let environment = trusted.environment(&home, &cargo, &git, &inherited, token)?;
    run_fetches(&cargo, &environment, &args.manifest_path, token.is_some())
}

fn api_client(token: String) -> Result<impl GitHubWrite, CiError> {
    let api = std::env::var(API_VARIABLE)
        .ok()
        .filter(|api| api.starts_with("https://") || api.starts_with("http://127.0.0.1"))
        .ok_or_else(|| CiError::new(format!("set {API_VARIABLE} to github.api_url")))?;
    Ok(super::http::HttpGitHub::new(
        api,
        token,
        super::http::DEFAULT_TIMEOUT,
    ))
}

fn run_fetches(
    cargo: &Path,
    environment: &[(OsString, OsString)],
    manifests: &[PathBuf],
    with_token: bool,
) -> Result<(), CiError> {
    for manifest in manifests {
        let mut command = Command::new(cargo);
        command
            .env_clear()
            .envs(environment.iter().map(|(k, v)| (k, v)))
            .args(["fetch", "--locked"])
            .arg("--manifest-path")
            .arg(manifest);
        if !with_token {
            command.arg("--offline");
        }
        let status = command
            .status()
            .map_err(|e| CiError::new(format!("cannot run {}: {e}", cargo.display())))?;
        if !status.success() {
            let why = if with_token { "" } else { FORK_HINT };
            return Err(CiError::new(format!(
                "cargo fetch for {} failed with {status}{}{why}",
                manifest.display(),
                if with_token { "" } else { "; " }
            )));
        }
    }
    Ok(())
}

pub(super) struct Inherited {
    pub(super) cargo_home: Option<OsString>,
    pub(super) rustup_home: Option<OsString>,
}

pub(super) struct Trusted {
    roots: Vec<PathBuf>,
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

    fn within(&self, path: &Path) -> bool {
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
            if trusted_directory && resolved.is_file() {
                return Ok(candidate);
            }
        }
        Err(CiError::new(format!(
            "no {name} in a tool directory the runner installed; the checkout cannot supply it"
        )))
    }

    pub(super) fn environment(
        &self,
        home: &Path,
        cargo: &Path,
        git: &Path,
        inherited: &Inherited,
        token: Option<&str>,
    ) -> Result<Vec<(OsString, OsString)>, CiError> {
        let home_variable = |value: &Option<OsString>, default: PathBuf, name: &str| match value
            .as_ref()
            .filter(|value| !value.is_empty())
        {
            None => Ok(default),
            Some(value) => {
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
        };
        let cargo_home = home_variable(&inherited.cargo_home, home.join(".cargo"), "CARGO_HOME")?;
        let rustup_home =
            home_variable(&inherited.rustup_home, home.join(".rustup"), "RUSTUP_HOME")?;
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
        if let Some(token) = token {
            environment.extend([
                ("GIT_CONFIG_COUNT".into(), "1".into()),
                (
                    "GIT_CONFIG_KEY_0".into(),
                    format!("url.https://x-access-token:{token}@github.com/.insteadOf").into(),
                ),
                ("GIT_CONFIG_VALUE_0".into(), "https://github.com/".into()),
            ]);
        }
        Ok(environment)
    }
}

pub(super) fn search_path() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default()
}

fn account_home() -> Result<PathBuf, CiError> {
    nix::unistd::User::from_uid(nix::unistd::Uid::current())
        .ok()
        .flatten()
        .map(|user| user.dir)
        .filter(|dir| dir.is_absolute())
        .ok_or_else(|| CiError::new("cannot read the home directory of the runner account"))
}
