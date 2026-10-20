//! Credential session for the private fetch (HD-7): the only code that
//! reaches the private-fetch spawn policy for fetching.
//!
//! The token never enters a child environment or argv. It lives in a mode-0600
//! file inside a mode-0700 Velnor-owned directory under runner temp, read by a
//! git credential helper keyed exactly on `https://github.com/<owner>/<repo>`
//! and by `curl --config`; the directory is removed on drop and before the
//! revoke step. The workspace forbids `unsafe`, which rules out inheriting a
//! pipe descriptor into `git`, so a file stands in for the design's pipe.

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use velnor_actions_mise::command::IsolatedCommand;
use velnor_actions_mise::{MISE_GLOBAL_FLAGS, PinnedTool, ToolCatalog};

use crate::OrchestratorError;
use crate::internal::internal;

/// GitHub REST API origin; every request is built from this constant.
pub(crate) const API_ORIGIN: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";
const CURL_TIMEOUT_SECS: u64 = 900;
const CARGO_TIMEOUT: Duration = Duration::from_mins(30);
const CAPTURE_BYTES: usize = 1024 * 1024;
const GIT_USER: &str = "x-access-token";

/// An installation token that never prints.
pub(crate) struct Token(String);

impl Token {
    /// Validate the token charset (`ghs_` tokens are alphanumeric, `_`, `-`, `.`).
    pub(crate) fn new(value: &str) -> Result<Self, OrchestratorError> {
        if value.is_empty() {
            return Err(internal("private_source_credential_unavailable"));
        }
        if value.len() > 512
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        {
            return Err(internal("private_source_credential_malformed"));
        }
        Ok(Self(value.to_owned()))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

/// Path characters a generated file reference may carry (no quoting needed).
fn is_plain_path(path: &Path) -> bool {
    path.is_absolute()
        && path.to_str().is_some_and(|text| {
            text.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'_' | b'-' | b'.'))
        })
}

/// Velnor-owned git configuration: one credential section per allowlisted
/// repository URL (with and without the `.git` suffix), nothing else.
///
/// # Errors
/// Rejects a token path that would need quoting.
pub(crate) fn git_config(
    owner: &str,
    repositories: &[String],
    token_file: &Path,
) -> Result<String, OrchestratorError> {
    if !is_plain_path(token_file) {
        return Err(internal("private_token_path_unsafe"));
    }
    let helper = format!(
        "!f() {{ test \\\"$1\\\" = get && printf 'username={GIT_USER}\\\\npassword=%s\\\\n' \\\"$(cat {})\\\"; }}; f",
        token_file.display()
    );
    let mut lines = vec!["[credential]".to_owned(), "\tuseHttpPath = true".to_owned()];
    for repository in repositories {
        for suffix in ["", ".git"] {
            lines.push(format!(
                "[credential \"https://github.com/{owner}/{repository}{suffix}\"]"
            ));
            lines.push(format!("\thelper = \"{helper}\""));
        }
    }
    lines.push(String::new());
    let text = lines.join("\n");
    Ok(text)
}

/// `curl --config` document for one authenticated request.
///
/// `method` is `GET` or `DELETE`; `output` is a file path or `/dev/null`.
/// Redirects stay on https and curl drops the `Authorization` header when a
/// redirect leaves the API host.
///
/// # Errors
/// Rejects a route or output path outside the safe grammar.
pub(crate) fn curl_config(
    token: &Token,
    method: &str,
    route: &str,
    accept: &str,
    output: &Path,
    write_status: bool,
) -> Result<String, OrchestratorError> {
    let plain_route = route
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-._~/%".contains(&b))
        && !route.starts_with('/')
        && !route.contains("..");
    let plain_accept = accept
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-._+/".contains(&b));
    if !plain_route || !plain_accept || !matches!(method, "GET" | "DELETE") {
        return Err(internal("private_fetch_request_unsafe"));
    }
    if output != Path::new("/dev/null") && !is_plain_path(output) {
        return Err(internal("private_fetch_output_unsafe"));
    }
    let mut lines = vec![
        "silent".to_owned(),
        "show-error".to_owned(),
        "fail-with-body".to_owned(),
        "location".to_owned(),
        "proto = \"=https\"".to_owned(),
        "proto-redir = \"=https\"".to_owned(),
        format!("max-time = {CURL_TIMEOUT_SECS}"),
        "retry = 2".to_owned(),
        format!("request = \"{method}\""),
        format!("header = \"Authorization: Bearer {}\"", token.expose()),
        format!("header = \"Accept: {accept}\""),
        format!("header = \"X-GitHub-Api-Version: {API_VERSION}\""),
        format!("output = \"{}\"", output.display()),
    ];
    if write_status {
        lines.push("write-out = \"%{http_code}\"".to_owned());
    }
    lines.push(format!("url = \"{API_ORIGIN}/{route}\""));
    lines.push(String::new());
    let text = lines.join("\n");
    Ok(text)
}

fn write_private(path: &Path, text: &str) -> Result<(), OrchestratorError> {
    let io =
        |err: std::io::Error| OrchestratorError::io(path.display().to_string(), err.to_string());
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path).map_err(io)?;
    file.write_all(text.as_bytes()).map_err(io)
}

fn create_private_dir(path: &Path) -> Result<(), OrchestratorError> {
    let io =
        |err: std::io::Error| OrchestratorError::io(path.display().to_string(), err.to_string());
    fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new("."))).map_err(io)?;
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path).map_err(io)
}

/// Remove a credential directory, overwriting every file first (best effort).
pub(crate) fn scrub_dir(path: &Path) -> Result<(), OrchestratorError> {
    let io =
        |err: std::io::Error| OrchestratorError::io(path.display().to_string(), err.to_string());
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let file = entry.path();
            if let Ok(metadata) = fs::symlink_metadata(&file)
                && metadata.is_file()
                && let Ok(mut handle) = fs::OpenOptions::new().write(true).open(&file)
            {
                let zeros = vec![0_u8; usize::try_from(metadata.len()).unwrap_or(0)];
                if handle.write_all(&zeros).is_err() {
                    // Best effort: removal below is what matters.
                }
            }
        }
    }
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(io(err)),
    }
}

/// A live credential directory; removed on drop.
pub(crate) struct Session {
    dir: PathBuf,
    token: Token,
    owned_env: Vec<(String, String)>,
    catalog: ToolCatalog,
    runner_temp: PathBuf,
    requests: std::cell::Cell<u32>,
}

impl Session {
    /// Create the directory, token file and git configuration.
    ///
    /// # Errors
    /// Fails if the directory cannot be created exclusively or a path is unsafe.
    pub(crate) fn create(
        runner_temp: &Path,
        token: Token,
        owner: &str,
        repositories: &[String],
        owned_env: Vec<(String, String)>,
        catalog: ToolCatalog,
    ) -> Result<Self, OrchestratorError> {
        let dir = runner_temp.join("velnor").join("private-fetch");
        scrub_dir(&dir)?;
        create_private_dir(&dir)?;
        let session = Self {
            dir,
            token,
            owned_env,
            catalog,
            runner_temp: runner_temp.to_owned(),
            requests: std::cell::Cell::new(0),
        };
        write_private(&session.token_file(), session.token.expose())?;
        write_private(
            &session.git_config_file(),
            &git_config(owner, repositories, &session.token_file())?,
        )?;
        Ok(session)
    }

    pub(crate) fn token_file(&self) -> PathBuf {
        self.dir.join("token")
    }

    pub(crate) fn git_config_file(&self) -> PathBuf {
        self.dir.join("gitconfig")
    }

    /// The `cargo fetch --locked` child for one manifest.
    ///
    /// Runs from the runner-owned isolation directory so no repository
    /// `.cargo/config.toml` is read, with git configured through the Velnor
    /// file only. `cargo fetch` runs no build scripts.
    pub(crate) fn cargo_fetch_command(
        &self,
        manifest: &Path,
    ) -> Result<IsolatedCommand, OrchestratorError> {
        let isolation = self.runner_temp.join("velnor").join("cargo-clean");
        fs::create_dir_all(&isolation).map_err(|err| {
            OrchestratorError::io(isolation.display().to_string(), err.to_string())
        })?;
        let mut args: Vec<OsString> = MISE_GLOBAL_FLAGS.iter().map(OsString::from).collect();
        args.push(OsString::from("exec"));
        args.push(OsString::from(self.catalog.tool_spec(PinnedTool::Rust)));
        args.extend(
            ["--", "cargo", "fetch", "--locked", "--manifest-path"]
                .iter()
                .map(OsString::from),
        );
        args.push(manifest.as_os_str().to_owned());
        IsolatedCommand::private_fetch(
            "mise",
            args,
            isolation,
            &self.git_config_file(),
            &self.owned_env,
            None,
        )
        .map_err(|err| internal(&format!("private_cargo_fetch_command:{err}")))
    }

    /// Run `cargo fetch --locked` for one manifest.
    pub(crate) fn cargo_fetch(&self, manifest: &Path) -> Result<(), OrchestratorError> {
        let output = self
            .cargo_fetch_command(manifest)?
            .run_bounded(CAPTURE_BYTES * 8, CARGO_TIMEOUT)
            .map_err(|err| internal(&format!("private_cargo_fetch_spawn:{err}")))?;
        if !output.success {
            eprintln!("{}", String::from_utf8_lossy(&output.stderr));
            return Err(internal("private_cargo_fetch_failed"));
        }
        Ok(())
    }

    fn curl(&self, text: &str) -> Result<velnor_actions_mise::ProcessOutput, OrchestratorError> {
        let index = self.requests.get();
        self.requests.set(index + 1);
        let file = self.dir.join(format!("curl-{index}.conf"));
        write_private(&file, text)?;
        let command = IsolatedCommand::private_fetch(
            "curl",
            vec![OsString::from("--config"), file.as_os_str().to_owned()],
            self.dir.clone(),
            &self.git_config_file(),
            &[],
            None,
        )
        .map_err(|err| internal(&format!("private_curl_command:{err}")))?;
        let result = command
            .run_bounded(CAPTURE_BYTES, Duration::from_secs(CURL_TIMEOUT_SECS + 60))
            .map_err(|err| internal(&format!("private_curl_spawn:{err}")));
        drop(fs::remove_file(&file));
        result
    }

    /// Authenticated GET of one API route into `dest`.
    pub(crate) fn download(
        &self,
        route: &str,
        accept: &str,
        dest: &Path,
    ) -> Result<(), OrchestratorError> {
        let text = curl_config(&self.token, "GET", route, accept, dest, false)?;
        let output = self.curl(&text)?;
        if output.success {
            Ok(())
        } else {
            Err(internal(&format!("private_download_failed:{route}")))
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        drop(scrub_dir(&self.dir));
    }
}

/// HTTP status of one authenticated request, body discarded.
///
/// # Errors
/// Fails when curl cannot run or prints no numeric status.
pub(crate) fn http_status(
    runner_temp: &Path,
    token: &Token,
    method: &str,
    route: &str,
) -> Result<u16, OrchestratorError> {
    let dir = runner_temp.join("velnor").join("private-revoke");
    scrub_dir(&dir)?;
    create_private_dir(&dir)?;
    let result = (|| {
        let file = dir.join("curl.conf");
        let text = curl_config(
            token,
            method,
            route,
            "application/vnd.github+json",
            Path::new("/dev/null"),
            true,
        )?
        .replace("fail-with-body\n", "");
        write_private(&file, &text)?;
        let command = IsolatedCommand::private_fetch(
            "curl",
            vec![OsString::from("--config"), file.as_os_str().to_owned()],
            dir.clone(),
            &dir.join("unused-gitconfig"),
            &[],
            None,
        )
        .map_err(|err| internal(&format!("private_curl_command:{err}")))?;
        let output = command
            .run_bounded(CAPTURE_BYTES, Duration::from_secs(CURL_TIMEOUT_SECS))
            .map_err(|err| internal(&format!("private_curl_spawn:{err}")))?;
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u16>()
            .map_err(|_| internal("private_http_status_unreadable"))
    })();
    let cleanup = scrub_dir(&dir);
    let status = result?;
    cleanup?;
    Ok(status)
}

#[cfg(test)]
#[path = "private_fetch_session_tests.rs"]
mod tests;
