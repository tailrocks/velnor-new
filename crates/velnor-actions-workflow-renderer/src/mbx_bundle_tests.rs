//! MBX bundle key and restore-prefix regressions.

use std::error::Error;
use std::process::Command;

use super::KEY_SCRIPT;

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
    key: &str,
    job_id: &str,
    matrix_key: &str,
    run_id: &str,
    run_attempt: &str,
) -> std::io::Result<String> {
    let output = Command::new("bash")
        .args(["-c", KEY_SCRIPT])
        .env("MBX_KEY", key)
        .env("MBX_JOB_ID", job_id)
        .env("MBX_MATRIX_KEY", matrix_key)
        .env("MBX_RUN_ID", run_id)
        .env("MBX_RUN_ATTEMPT", run_attempt)
        .env("GITHUB_OUTPUT", "/dev/stdout")
        .output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    String::from_utf8(output.stdout).map_err(std::io::Error::other)
}

#[test]
fn writers_are_unique_per_run_and_restore_by_stable_compatible_prefix() -> Result<(), Box<dyn Error>>
{
    let action_key =
        "linux-x64-mbx-generation-rust-1.98.1-0123456789abcdef0123456789abcdef01234567";
    let next_commit_key =
        "linux-x64-mbx-generation-rust-1.98.1-fedcba9876543210fedcba9876543210fedcba98";
    let hosted = run_key_script(action_key, "rust-demo__hosted", "", "100", "1")?;
    let local = run_key_script(action_key, "rust-demo__local", "", "100", "1")?;
    let hosted_next_run = run_key_script(next_commit_key, "rust-demo__hosted", "", "101", "1")?;
    let hosted_rerun = run_key_script(action_key, "rust-demo__hosted", "", "100", "2")?;
    let matrix_a = run_key_script(action_key, "velnor-task", "m-0123456789abcdef", "100", "1")?;
    let matrix_b = run_key_script(action_key, "velnor-task", "m-fedcba9876543210", "100", "1")?;
    let matrix_a_next_run = run_key_script(
        next_commit_key,
        "velnor-task",
        "m-0123456789abcdef",
        "101",
        "1",
    )?;
    let matrix_a_second_reader = run_key_script(
        next_commit_key,
        "velnor-task",
        "m-0123456789abcdef",
        "102",
        "1",
    )?;
    let common_prefix = "linux-x64-mbx-generation-rust-1.98.1-";
    let compatibility_key = "linux-x64-mbx-generation-rust-1.98.1";
    let hosted_stable_prefix = format!("{compatibility_key}-rust-demo__hosted-");
    let matrix_a_stable_prefix = format!("{compatibility_key}-velnor-task-m-0123456789abcdef-");

    assert_eq!(
        output_for(&hosted, "key"),
        Some("linux-x64-mbx-generation-rust-1.98.1-rust-demo__hosted-r100-a1")
    );
    assert_eq!(
        output_for(&local, "key"),
        Some("linux-x64-mbx-generation-rust-1.98.1-rust-demo__local-r100-a1")
    );
    assert_eq!(
        output_for(&matrix_a, "key"),
        Some("linux-x64-mbx-generation-rust-1.98.1-velnor-task-m-0123456789abcdef-r100-a1")
    );
    assert_eq!(
        output_for(&matrix_b, "key"),
        Some("linux-x64-mbx-generation-rust-1.98.1-velnor-task-m-fedcba9876543210-r100-a1")
    );
    assert_ne!(output_for(&hosted, "key"), output_for(&local, "key"));
    assert_ne!(
        output_for(&hosted, "key"),
        output_for(&hosted_next_run, "key")
    );
    assert_ne!(output_for(&hosted, "key"), output_for(&hosted_rerun, "key"));
    assert_ne!(output_for(&matrix_a, "key"), output_for(&matrix_b, "key"));
    assert_ne!(
        output_for(&matrix_a, "key"),
        output_for(&matrix_a_next_run, "key")
    );
    assert_ne!(
        output_for(&matrix_a_next_run, "key"),
        output_for(&matrix_a_second_reader, "key")
    );
    assert_eq!(
        output_for(&hosted, "prefix"),
        Some(hosted_stable_prefix.as_str())
    );
    for reader in [&hosted_next_run, &hosted_rerun] {
        assert_eq!(
            output_for(reader, "prefix"),
            Some(hosted_stable_prefix.as_str())
        );
    }
    for reader in [&matrix_a, &matrix_a_next_run, &matrix_a_second_reader] {
        assert_eq!(
            output_for(reader, "prefix"),
            Some(matrix_a_stable_prefix.as_str())
        );
    }
    for output in [
        &hosted,
        &local,
        &hosted_next_run,
        &hosted_rerun,
        &matrix_a,
        &matrix_b,
        &matrix_a_next_run,
        &matrix_a_second_reader,
    ] {
        assert_eq!(output_for(output, "fallback"), Some(common_prefix));
    }
    let previous_hosted_key = output_for(&hosted, "key").expect("previous hosted key");
    for reader in [&hosted_next_run, &hosted_rerun] {
        assert!(
            previous_hosted_key
                .starts_with(output_for(reader, "prefix").expect("hosted reader prefix"))
        );
    }
    let previous_matrix_key = output_for(&matrix_a, "key").expect("previous matrix key");
    for reader in [&matrix_a_next_run, &matrix_a_second_reader] {
        assert!(
            previous_matrix_key
                .starts_with(output_for(reader, "prefix").expect("matrix reader prefix"))
        );
    }
    Ok(())
}

#[test]
fn fresh_runs_restore_previous_compatible_payload_before_common_fallback()
-> Result<(), Box<dyn Error>> {
    let action_key =
        "linux-x64-mbx-generation-rust-1.98.1-0123456789abcdef0123456789abcdef01234567";
    let prior_action_key =
        "linux-x64-mbx-generation-rust-1.98.1-0123456789abcdef0123456789abcdef01234567";
    let next_commit_action_key =
        "linux-x64-mbx-generation-rust-1.98.1-fedcba9876543210fedcba9876543210fedcba98";
    let prior_writer = run_key_script(
        prior_action_key,
        "velnor-task",
        "m-0123456789abcdef",
        "100",
        "1",
    )?;
    let fresh_reader = run_key_script(
        next_commit_action_key,
        "velnor-task",
        "m-0123456789abcdef",
        "101",
        "1",
    )?;
    let rerun_reader = run_key_script(action_key, "velnor-task", "m-0123456789abcdef", "100", "2")?;
    let prior_key = output_for(&prior_writer, "key").expect("prior writer key");
    let common_key = "linux-x64-mbx-generation-rust-1.98.1-older-snapshot";
    let entries = [
        (prior_key, "same matrix closure objects"),
        (common_key, "common objects"),
    ];

    for reader in [&fresh_reader, &rerun_reader] {
        let primary = output_for(reader, "key").expect("reader primary key");
        let prefixes = [
            output_for(reader, "prefix").expect("reader stable prefix"),
            output_for(reader, "fallback").expect("reader common fallback"),
        ];
        assert_eq!(
            restore_payload(&entries, primary, &prefixes),
            Some((false, "same matrix closure objects")),
            "reader must restore the prior compatible payload by its stable prefix"
        );
    }
    Ok(())
}

#[test]
fn invalid_run_identity_fails_closed() {
    let key = "linux-x64-mbx-v1-rust-1.98.1-0123456789abcdef";
    assert!(run_key_script(key, "rust-demo__hosted", "", "100/1", "1").is_err());
    assert!(run_key_script(key, "rust-demo__hosted", "", "100", "1a").is_err());
}
