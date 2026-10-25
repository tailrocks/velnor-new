//! Bounded subprocess helpers for failure-only CLI diagnostics.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::MiseDiagnosticContext;
use super::bounded_process::{BoundedOutput, BoundedProcessError};

const CAPTURE_LIMIT: usize = 8 * 1024;
const DISPLAY_LIMIT: usize = 4096;
const TRUNCATION_MARKER: &str = "…<truncated>";
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const TOOL_ENV_KEYS: [&str; 11] = super::tool_context::TOOL_ENV;
const VERIFY_CREDENTIAL_KEYS: [&str; 9] = [
    "MISE_GITHUB_TOKEN",
    "GITHUB_TOKEN",
    "GH_TOKEN",
    "ACTIONS_RUNTIME_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_URL",
    "CARGO_REGISTRY_TOKEN",
    "NPM_TOKEN",
    "NODE_AUTH_TOKEN",
];
const VERIFY_ENDPOINT_KEYS: [&str; 2] = ["GH_HOST", "GH_CONFIG_DIR"];
const VERIFY_FETCH_REDIRECT_KEYS: [&str; 4] = [
    "GIT_ASKPASS",
    "SSH_ASKPASS",
    "GIT_CONFIG",
    "BUN_CONFIG_REGISTRY",
];
const ISOLATION_ENV: [(&str, &str); 4] = [
    ("MISE_NO_CONFIG", "1"),
    ("MISE_NO_ENV", "1"),
    ("MISE_NO_HOOKS", "1"),
    ("MISE_LOCKFILE", "0"),
];
const NO_AUTO_INSTALL_ENV: [(&str, &str); 2] = [
    ("MISE_AUTO_INSTALL", "false"),
    ("MISE_EXEC_AUTO_INSTALL", "false"),
];
const RUST_PIN: &str = "1.99.1";

pub(super) fn selected_environment(
    context: MiseDiagnosticContext,
    explicit_env: &[(&str, &str)],
    parent: &[(OsString, OsString)],
) -> BTreeMap<OsString, OsString> {
    let mut selected = BTreeMap::new();
    match context {
        MiseDiagnosticContext::Inherited => {
            for (key, value) in parent {
                if !filtered_verify_key(key) {
                    selected.insert(key.clone(), value.clone());
                }
            }
        }
        MiseDiagnosticContext::PathOnly | MiseDiagnosticContext::PathAndToolHomes => {
            if let Some((_, path)) = parent.iter().find(|(key, _)| key == "PATH") {
                selected.insert(OsString::from("PATH"), path.clone());
            }
            if matches!(context, MiseDiagnosticContext::PathAndToolHomes) {
                for key in TOOL_ENV_KEYS {
                    if let Some((_, value)) = parent
                        .iter()
                        .rev()
                        .find(|(parent_key, value)| parent_key == key && !value.is_empty())
                    {
                        selected.insert(OsString::from(key), value.clone());
                    }
                }
            }
        }
    }
    for (key, value) in explicit_env {
        selected.insert(OsString::from(key), OsString::from(value));
    }
    for (key, value) in ISOLATION_ENV.into_iter().chain(NO_AUTO_INSTALL_ENV) {
        selected.insert(OsString::from(key), OsString::from(value));
    }
    selected
}

fn filtered_verify_key(key: &OsStr) -> bool {
    let name = key.to_string_lossy();
    VERIFY_CREDENTIAL_KEYS.contains(&name.as_ref())
        || name.starts_with("CARGO_REGISTRIES_")
        || name.ends_with("_TOKEN")
        || VERIFY_ENDPOINT_KEYS.contains(&name.as_ref())
        || VERIFY_FETCH_REDIRECT_KEYS.contains(&name.as_ref())
        || name.starts_with("GIT_CONFIG_")
        || name.starts_with("NPM_CONFIG_")
        || name.starts_with("npm_config_")
        || name.starts_with("TF_")
        || name.starts_with("TOFU_")
        || name.starts_with("CHECKPOINT_")
}

pub(super) fn run_bounded(
    program: &OsStr,
    args: &[OsString],
    cwd: &Path,
    environment: &BTreeMap<OsString, OsString>,
    capture_limit: usize,
    timeout: Duration,
) -> Result<BoundedOutput, BoundedProcessError> {
    let mut env_args = vec![OsString::from("-i")];
    env_args.extend(
        environment
            .iter()
            .map(|(key, value)| assignment(key, value)),
    );
    env_args.push(program.to_owned());
    env_args.extend_from_slice(args);
    let mut command = Command::new("/usr/bin/env");
    command.env_clear().args(env_args).current_dir(cwd);
    super::bounded_process::run_bounded(&mut command, capture_limit, timeout)
}

pub(super) fn run_probe(
    program: &OsStr,
    args: &[OsString],
    cwd: &Path,
    environment: &BTreeMap<OsString, OsString>,
) -> Result<BoundedOutput, BoundedProcessError> {
    run_bounded(
        program,
        args,
        cwd,
        environment,
        CAPTURE_LIMIT,
        PROBE_TIMEOUT,
    )
}

fn assignment(key: &OsStr, value: &OsStr) -> OsString {
    let mut assignment = key.to_owned();
    assignment.push("=");
    assignment.push(value);
    assignment
}

pub(super) fn command_summary(result: &BoundedOutput) -> String {
    format!(
        "success={} code={:?} signal={:?} stdout={:?} stderr={:?}",
        result.success,
        result.code,
        result.signal,
        bounded_text(&result.stdout),
        bounded_text(&result.stderr)
    )
}

fn bounded_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let truncated = text.len() > DISPLAY_LIMIT;
    let limit = if truncated {
        DISPLAY_LIMIT - TRUNCATION_MARKER.len()
    } else {
        DISPLAY_LIMIT
    };
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut displayed = text[..end].to_owned();
    if truncated {
        displayed.push_str(TRUNCATION_MARKER);
    }
    displayed
}

pub(super) fn rustup_path(environment: &BTreeMap<OsString, OsString>) -> Option<OsString> {
    let installs = env_path(environment, "MISE_INSTALLS_DIR")
        .or_else(|| env_path(environment, "MISE_DATA_DIR").map(|path| path.join("installs")))
        .or_else(|| env_path(environment, "XDG_DATA_HOME").map(|path| path.join("mise/installs")))
        .or_else(|| {
            env_path(environment, "HOME").map(|path| path.join(".local/share/mise/installs"))
        })?;
    Some(
        installs
            .join("rust")
            .join(RUST_PIN)
            .join("rustup")
            .into_os_string(),
    )
}

fn env_path(environment: &BTreeMap<OsString, OsString>, key: &str) -> Option<std::path::PathBuf> {
    environment
        .get(OsStr::new(key))
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::time::Instant;

    fn pairs(values: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        values
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value)))
            .collect()
    }

    fn run_shell(
        script: &str,
        cap: usize,
        timeout: Duration,
    ) -> Result<BoundedOutput, BoundedProcessError> {
        let environment =
            BTreeMap::from([(OsString::from("PATH"), OsString::from("/bin:/usr/bin"))]);
        run_bounded(
            OsStr::new("/bin/sh"),
            &[OsString::from("-c"), OsString::from(script)],
            Path::new("/"),
            &environment,
            cap,
            timeout,
        )
    }

    #[test]
    fn diagnostic_environments_preserve_only_their_intended_tool_context() {
        let parent = pairs(&[
            ("PATH", "/tools/bin"),
            ("HOME", "/home/runner"),
            ("MISE_INSTALLS_DIR", "/tools/installs"),
            ("MISE_CONFIG_DIR", "/config"),
            ("MISE_CACHE_DIR", ""),
            ("GITHUB_TOKEN", "must-not-pass"),
        ]);
        let path_only = selected_environment(MiseDiagnosticContext::PathOnly, &[], &parent);
        assert_eq!(
            path_only.get(OsStr::new("PATH")).map(OsString::as_os_str),
            Some(OsStr::new("/tools/bin"))
        );
        assert!(!path_only.contains_key(OsStr::new("HOME")));
        assert!(!path_only.contains_key(OsStr::new("MISE_INSTALLS_DIR")));

        let tool_homes = selected_environment(
            MiseDiagnosticContext::PathAndToolHomes,
            &[("MISE_DATA_DIR", "/override")],
            &parent,
        );
        assert_eq!(
            tool_homes.get(OsStr::new("HOME")).map(OsString::as_os_str),
            Some(OsStr::new("/home/runner"))
        );
        assert_eq!(
            tool_homes
                .get(OsStr::new("MISE_INSTALLS_DIR"))
                .map(OsString::as_os_str),
            Some(OsStr::new("/tools/installs"))
        );
        assert!(!tool_homes.contains_key(OsStr::new("MISE_CACHE_DIR")));
        assert_eq!(
            tool_homes
                .get(OsStr::new("MISE_DATA_DIR"))
                .map(OsString::as_os_str),
            Some(OsStr::new("/override"))
        );

        let inherited = selected_environment(MiseDiagnosticContext::Inherited, &[], &parent);
        assert_eq!(
            inherited
                .get(OsStr::new("MISE_CONFIG_DIR"))
                .map(OsString::as_os_str),
            Some(OsStr::new("/config"))
        );
        assert!(!inherited.contains_key(OsStr::new("GITHUB_TOKEN")));
        assert_eq!(
            inherited
                .get(OsStr::new("MISE_AUTO_INSTALL"))
                .map(OsString::as_os_str),
            Some(OsStr::new("false"))
        );
    }

    #[test]
    fn bounded_probe_preserves_status_and_both_streams() -> Result<(), Box<dyn Error>> {
        let result = run_shell(
            "printf probe-out; printf probe-err >&2; exit 7",
            CAPTURE_LIMIT,
            Duration::from_secs(2),
        )?;
        assert!(!result.success);
        assert_eq!(result.code, Some(7));
        assert_eq!(result.signal, None);
        assert_eq!(result.stdout, b"probe-out");
        assert_eq!(result.stderr, b"probe-err");
        assert!(command_summary(&result).contains("code=Some(7)"));
        Ok(())
    }

    #[test]
    fn bounded_probe_uses_the_selected_environment_and_marks_display_truncation()
    -> Result<(), Box<dyn Error>> {
        let parent = pairs(&[("PATH", "/bin:/usr/bin"), ("HOME", "/fixture-home")]);
        let environment = selected_environment(
            MiseDiagnosticContext::PathOnly,
            &[("DIAGNOSTIC_FIXTURE", "selected")],
            &parent,
        );
        let result = run_bounded(
            OsStr::new("/bin/sh"),
            &[
                OsString::from("-c"),
                OsString::from("printf '%s:%s' \"$DIAGNOSTIC_FIXTURE\" \"$MISE_AUTO_INSTALL\""),
            ],
            Path::new("/"),
            &environment,
            CAPTURE_LIMIT,
            Duration::from_secs(2),
        )?;
        assert_eq!(result.stdout, b"selected:false");

        let long = run_shell("printf '%05000d' 0", CAPTURE_LIMIT, Duration::from_secs(2))?;
        let summary = command_summary(&long);
        assert!(summary.contains(TRUNCATION_MARKER));
        assert!(summary.len() < DISPLAY_LIMIT + 100);
        Ok(())
    }

    #[test]
    fn bounded_probe_stops_on_deadline_and_output_limit() {
        let started = Instant::now();
        let timeout = run_shell("sleep 5", CAPTURE_LIMIT, Duration::from_millis(100));
        assert!(matches!(timeout, Err(BoundedProcessError::TimedOut)));
        assert!(started.elapsed() < Duration::from_secs(2));

        let oversized = run_shell("printf 0123456789", 4, Duration::from_secs(2));
        assert!(
            oversized.is_err(),
            "output over the capture cap must fail closed"
        );
    }
}
