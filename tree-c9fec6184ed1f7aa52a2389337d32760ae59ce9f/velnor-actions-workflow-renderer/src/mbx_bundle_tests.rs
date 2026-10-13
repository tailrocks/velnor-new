//! MBX bundle key and restore-prefix regressions.

use std::error::Error;
use std::process::{Command, Output};

use super::KEY_SCRIPT;

const GENERATION: &str = "velnor-mbx-1.21.1-dir";
const COMPATIBILITY: &str = "linux-x64-mbx-velnor-mbx-1.21.1-dir-rust-eaa76ad37f36";

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
