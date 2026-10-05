//! MBX bundle key and restore-prefix regressions.

use std::collections::BTreeMap;
use std::error::Error;
use std::process::{Command, Output};

use velnor_actions_contract::{Step, StepKind};

use super::KEY_SCRIPT;

const GENERATION: &str = "velnor-mbx-1.21.1-dir";
const ACTION_GENERATION: &str = "velnor-mbx-1.21.1";
const COMPATIBILITY: &str = "linux-x64-mbx-velnor-mbx-1.21.1-dir-rust-eaa76ad37f36";
const TOOL_HOME_ENV_KEYS: [&str; 5] = [
    "MISE_RUSTUP_HOME",
    "MISE_CARGO_HOME",
    "RUSTUP_TOOLCHAIN",
    "RUSTUP_HOME",
    "CARGO_HOME",
];

fn output_for<'a>(script_output: &'a str, name: &str) -> Option<&'a str> {
    script_output
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{name}=")))
}

fn restore_payload<'a>(
    entries: &'a [(&'a str, &'a str)],
    primary: &str,
    prefixes: &[&str],
) -> Option<(bool, &'a str)> {
    if let Some(payload) = entries
        .iter()
        .find_map(|(key, payload)| (*key == primary).then_some(*payload))
    {
        return Some((true, payload));
    }
    prefixes.iter().find_map(|prefix| {
        entries
            .iter()
            .rev()
            .find_map(|(key, payload)| key.starts_with(prefix).then_some(*payload))
            .map(|payload| (false, payload))
    })
}

fn run_key_script(
    job_id: &str,
    matrix_key: &str,
    run_id: &str,
    run_attempt: &str,
) -> std::io::Result<String> {
    let output =
        run_key_script_with_output(job_id, matrix_key, run_id, run_attempt, "/dev/stdout")?;
    if !output.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    String::from_utf8(output.stdout).map_err(std::io::Error::other)
}

fn run_key_script_with_output(
    job_id: &str,
    matrix_key: &str,
    run_id: &str,
    run_attempt: &str,
    github_output: &str,
) -> std::io::Result<Output> {
    let script = format!(
        r#"rustc() {{
  [ "$1" = "+1.98.1" ] && [ "$2" = "-vV" ] || return 1
  printf '%s' 'rustc 1.98.1
host: x86_64-unknown-linux-gnu'
}}
{KEY_SCRIPT}"#
    );
    Command::new("bash")
        .args(["-c", &script])
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("MBX_GENERATION", GENERATION)
        .env("MBX_TOOLCHAIN", "1.98.1")
        .env("MBX_JOB_ID", job_id)
        .env("MBX_MATRIX_KEY", matrix_key)
        .env("MBX_RUN_ID", run_id)
        .env("MBX_RUN_ATTEMPT", run_attempt)
        .env("GITHUB_OUTPUT", github_output)
        .output()
}

fn run_key_script_with_owned_homes(home_env: &BTreeMap<String, String>) -> std::io::Result<Output> {
    let script = format!(
        r#"rustc() {{
  [ "$1" = "+1.98.1" ] && [ "$2" = "-vV" ] || return 1
  [ "${{MISE_RUSTUP_HOME:-}}" = "/worker/mise/rustup" ] || return 1
  [ "${{MISE_CARGO_HOME:-}}" = "/worker/mise/cargo" ] || return 1
  [ "${{RUSTUP_TOOLCHAIN:-}}" = "1.98.1" ] || return 1
  [ "${{RUSTUP_HOME:-}}" = "/worker/rustup" ] || return 1
  [ "${{CARGO_HOME:-}}" = "/worker/cargo" ] || return 1
  printf '%s' 'rustc 1.98.1
host: x86_64-unknown-linux-gnu'
}}
{KEY_SCRIPT}"#
    );
    let mut command = Command::new("bash");
    command
        .args(["-c", &script])
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("MBX_GENERATION", GENERATION)
        .env("MBX_TOOLCHAIN", "1.98.1")
        .env("MBX_JOB_ID", "rust-app__local")
        .env("MBX_MATRIX_KEY", "")
        .env("MBX_RUN_ID", "100")
        .env("MBX_RUN_ATTEMPT", "1")
        .env("GITHUB_OUTPUT", "/dev/stdout");
    for key in TOOL_HOME_ENV_KEYS {
        if let Some(value) = home_env.get(key) {
            command.env(key, value);
        }
    }
    command.output()
}

fn mbx_action(env: BTreeMap<String, String>) -> Step {
    Step {
        name: "Restore MBX objects".to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: "jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6".to_owned(),
            with: BTreeMap::from([
                ("cache-generation".to_owned(), ACTION_GENERATION.to_owned()),
                ("toolchain".to_owned(), "1.98.1".to_owned()),
            ]),
            env,
        },
    }
}

#[test]
fn writers_match_action_compatibility_and_are_unique_per_run() -> Result<(), Box<dyn Error>> {
    let hosted = run_key_script("rust-demo__hosted", "", "100", "1")?;
    let local = run_key_script("rust-demo__local", "", "100", "1")?;
    let hosted_next_run = run_key_script("rust-demo__hosted", "", "101", "1")?;
    let hosted_rerun = run_key_script("rust-demo__hosted", "", "100", "2")?;
    let matrix = run_key_script("velnor-task", "m-0123456789abcdef", "100", "1")?;
    let matrix_next_run = run_key_script("velnor-task", "m-0123456789abcdef", "101", "1")?;

    let hosted_prefix = format!("{COMPATIBILITY}-j17-rust-demo__hosted-n-");
    let local_prefix = format!("{COMPATIBILITY}-j16-rust-demo__local-n-");
    let matrix_prefix = format!("{COMPATIBILITY}-j11-velnor-task-m-0123456789abcdef-");
    let hosted_key = format!("{hosted_prefix}r100-a1");
    let local_key = format!("{local_prefix}r100-a1");
    let matrix_writer_key = format!("{matrix_prefix}r100-a1");
    assert_eq!(output_for(&hosted, "key"), Some(hosted_key.as_str()));
    assert_eq!(output_for(&local, "key"), Some(local_key.as_str()));
    assert_eq!(output_for(&matrix, "key"), Some(matrix_writer_key.as_str()));
    assert_ne!(output_for(&hosted, "key"), output_for(&local, "key"));
    assert_ne!(
        output_for(&hosted, "key"),
        output_for(&hosted_next_run, "key")
    );
    assert_ne!(output_for(&hosted, "key"), output_for(&hosted_rerun, "key"));
    assert_ne!(
        output_for(&matrix, "key"),
        output_for(&matrix_next_run, "key")
    );

    let fallback = format!("{COMPATIBILITY}-");
    for output in [
        &hosted,
        &local,
        &hosted_next_run,
        &hosted_rerun,
        &matrix,
        &matrix_next_run,
    ] {
        assert_eq!(output_for(output, "fallback"), Some(fallback.as_str()));
    }
    for reader in [&hosted_next_run, &hosted_rerun] {
        assert_eq!(output_for(reader, "prefix"), Some(hosted_prefix.as_str()));
    }
    assert_eq!(
        output_for(&matrix_next_run, "prefix"),
        Some(matrix_prefix.as_str())
    );
    Ok(())
}

#[test]
fn restore_order_separates_overlapping_jobs_and_matrix_mode() -> Result<(), Box<dyn Error>> {
    let app = run_key_script("rust-app", "", "100", "1")?;
    let app_extra = run_key_script("rust-app-extra", "", "100", "1")?;
    let matrix_app = run_key_script("rust-app", "m-0123456789abcdef", "100", "1")?;
    let app_reader = run_key_script("rust-app", "", "101", "1")?;
    let app_extra_reader = run_key_script("rust-app-extra", "", "101", "1")?;
    let matrix_reader = run_key_script("rust-app", "m-0123456789abcdef", "101", "1")?;

    let app_key = output_for(&app, "key").expect("app writer key");
    let app_extra_key = output_for(&app_extra, "key").expect("app-extra writer key");
    let matrix_key = output_for(&matrix_app, "key").expect("matrix writer key");
    let app_prefix = output_for(&app_reader, "prefix").expect("app reader prefix");
    let app_extra_prefix = output_for(&app_extra_reader, "prefix").expect("app-extra prefix");
    let matrix_prefix = output_for(&matrix_reader, "prefix").expect("matrix prefix");

    assert!(!app_extra_key.starts_with(app_prefix));
    assert!(!matrix_key.starts_with(app_prefix));
    assert!(!app_key.starts_with(app_extra_prefix));
    assert!(!app_key.starts_with(matrix_prefix));

    let entries = [
        (app_key, "nonmatrix app payload"),
        (matrix_key, "matrix app payload"),
        (app_extra_key, "app-extra payload"),
    ];
    for (reader, expected) in [
        (&app_reader, "nonmatrix app payload"),
        (&matrix_reader, "matrix app payload"),
        (&app_extra_reader, "app-extra payload"),
    ] {
        let prefixes = [
            output_for(reader, "prefix").expect("writer prefix"),
            output_for(reader, "fallback").expect("compatibility fallback"),
        ];
        assert_eq!(
            restore_payload(
                &entries,
                output_for(reader, "key").expect("reader key"),
                &prefixes
            ),
            Some((false, expected)),
            "the stable writer prefix must win before the common fallback"
        );
    }
    Ok(())
}

#[test]
fn new_run_and_rerun_restore_the_previous_compatible_payload() -> Result<(), Box<dyn Error>> {
    let prior_writer = run_key_script("velnor-task", "m-0123456789abcdef", "100", "1")?;
    let fresh_reader = run_key_script("velnor-task", "m-0123456789abcdef", "101", "1")?;
    let rerun_reader = run_key_script("velnor-task", "m-0123456789abcdef", "100", "2")?;
    let prior_key = output_for(&prior_writer, "key").expect("prior writer key");
    let common_key = format!("{COMPATIBILITY}-old-snapshot");
    let entries = [
        (prior_key, "same matrix closure objects"),
        (common_key.as_str(), "common objects"),
    ];

    for reader in [&fresh_reader, &rerun_reader] {
        let prefixes = [
            output_for(reader, "prefix").expect("reader writer prefix"),
            output_for(reader, "fallback").expect("reader compatibility prefix"),
        ];
        assert_eq!(
            restore_payload(
                &entries,
                output_for(reader, "key").expect("reader primary key"),
                &prefixes
            ),
            Some((false, "same matrix closure objects"))
        );
    }
    Ok(())
}

#[test]
fn invalid_writer_identity_fails_closed() -> Result<(), Box<dyn Error>> {
    for output in [
        run_key_script("rust-app", "", "100/1", "1")?,
        run_key_script("rust-app", "", "100", "1a")?,
        run_key_script("rust-app", "bad-matrix", "100", "1")?,
    ] {
        assert_eq!(output_for(&output, "ready"), Some("false"));
        assert_eq!(output_for(&output, "acceptance"), Some("cache_unavailable"));
        assert_eq!(output_for(&output, "key"), None);
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn failed_key_output_handoff_fails_the_step() -> Result<(), Box<dyn Error>> {
    let output = run_key_script_with_output("rust-app", "", "100", "1", "/dev/null/output")?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("handoff failed"));
    Ok(())
}

#[test]
fn key_probe_uses_action_owned_tool_homes_and_keeps_empty_defaults() -> Result<(), Box<dyn Error>> {
    let empty_default = super::key_step(false, &mbx_action(BTreeMap::new()))?;
    let StepKind::Shell { env, .. } = empty_default.kind else {
        return Err(std::io::Error::other("MBX key step is not a shell step").into());
    };
    assert!(TOOL_HOME_ENV_KEYS.iter().all(|key| !env.contains_key(*key)));

    let empty_homes = TOOL_HOME_ENV_KEYS
        .iter()
        .map(|key| ((*key).to_owned(), String::new()))
        .collect();
    let empty_values = super::key_step(false, &mbx_action(empty_homes))?;
    let StepKind::Shell { env, .. } = empty_values.kind else {
        return Err(std::io::Error::other("MBX key step is not a shell step").into());
    };
    assert!(TOOL_HOME_ENV_KEYS.iter().all(|key| !env.contains_key(*key)));

    let homes = BTreeMap::from([
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "/worker/mise/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "/worker/mise/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
        ("RUSTUP_HOME".to_owned(), "/worker/rustup".to_owned()),
        ("CARGO_HOME".to_owned(), "/worker/cargo".to_owned()),
    ]);
    let key_step = super::key_step(false, &mbx_action(homes.clone()))?;
    let StepKind::Shell { env, .. } = key_step.kind else {
        return Err(std::io::Error::other("MBX key step is not a shell step").into());
    };
    for (key, value) in &homes {
        assert_eq!(env.get(key), Some(value));
    }
    let probe_homes = TOOL_HOME_ENV_KEYS
        .iter()
        .filter_map(|key| {
            env.get(*key)
                .map(|value| ((*key).to_owned(), value.clone()))
        })
        .collect();
    let output = run_key_script_with_owned_homes(&probe_homes)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout)?;
    assert_eq!(output_for(&output, "ready"), Some("true"));
    Ok(())
}
