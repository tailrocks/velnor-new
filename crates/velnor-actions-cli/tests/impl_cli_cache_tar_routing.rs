//! Binary routing tests for the cache tar adapter aliases.

#![cfg(unix)]

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir};

const OP: &str = "cache-action-stage-tar-v1";

#[test]
fn normal_binary_name_keeps_the_public_cli_route() -> Result<(), Box<dyn Error>> {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-actions"))
        .arg("--version")
        .env("VELNOR_INTERNAL_OP", OP)
        .output()?;

    assert_eq!(code(&output), 0, "{:?}", output.stderr);
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("velnor-actions "));
    Ok(())
}

#[test]
fn tar_aliases_keep_version_available_without_workflow_context() -> Result<(), Box<dyn Error>> {
    let temp = fresh_tempdir("cache-tar-version")?;
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_velnor-actions"));

    for alias in ["tar", "gtar"] {
        let path = temp.join(alias);
        symlink(&binary, &path)?;
        let output = Command::new(&path).arg("--version").env_clear().output()?;
        assert_eq!(code(&output), 0, "alias={alias}: {:?}", output.stderr);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "tar (GNU tar) velnor-cache-adapter-v1\n"
        );
    }

    cleanup(&temp);
    Ok(())
}

#[test]
fn tar_archive_route_requires_workflow_context_and_valid_stage_inputs() -> Result<(), Box<dyn Error>>
{
    let temp = fresh_tempdir("cache-tar-gate")?;
    let temp = fs::canonicalize(temp)?;
    let path = temp.join("tar");
    symlink(env!("CARGO_BIN_EXE_velnor-actions"), &path)?;

    let missing_run = run(
        &path,
        &temp,
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("VELNOR_INTERNAL_OP", OP),
        ],
    )?;
    assert_ne!(code(&missing_run), 0);
    assert!(
        String::from_utf8_lossy(&missing_run.stderr).contains("cache_tar_runner_context_invalid")
    );

    let outside_actions = run(
        &path,
        &temp,
        &[
            ("GITHUB_ACTIONS", "false"),
            ("GITHUB_RUN_ID", "12"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("VELNOR_INTERNAL_OP", OP),
        ],
    )?;
    assert_ne!(code(&outside_actions), 0);
    assert!(
        String::from_utf8_lossy(&outside_actions.stderr)
            .contains("cache_tar_runner_context_invalid")
    );

    let wrong_operation = run(
        &path,
        &temp,
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_RUN_ID", "12"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("VELNOR_INTERNAL_OP", "normal-tools-cache-stage-path-v1"),
        ],
    )?;
    assert_ne!(code(&wrong_operation), 0);
    assert!(
        String::from_utf8_lossy(&wrong_operation.stderr)
            .contains("cache_tar_runner_context_invalid")
    );

    let missing_stage = run(
        &path,
        &temp,
        &[
            ("GITHUB_ACTIONS", "true"),
            ("GITHUB_RUN_ID", "12"),
            ("GITHUB_RUN_ATTEMPT", "1"),
            ("VELNOR_INTERNAL_OP", OP),
        ],
    )?;
    assert_ne!(code(&missing_stage), 0);
    assert!(String::from_utf8_lossy(&missing_stage.stderr).contains("cache_stage_input_missing"));

    cleanup(&temp);
    Ok(())
}

fn run(alias: &Path, runner_temp: &Path, envs: &[(&str, &str)]) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(alias);
    command
        .args(["-tf", "cache.tar"])
        .env_clear()
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_WORKSPACE", runner_temp)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    for (name, value) in envs {
        command.env(name, value);
    }
    Ok(command.output()?)
}

#[test]
fn cache_tar_binary_reports_each_sdk_compression_identity() -> Result<(), Box<dyn Error>> {
    let temp = fs::canonicalize(fresh_tempdir("cache-tar-compression")?)?;
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_velnor-actions"));
    let alias = temp.join("tar");
    symlink(&binary, &alias)?;

    let cases = [
        CompressionCase {
            suffix: 'a',
            output: "gzip",
            args: &["-z"],
            zstd: false,
        },
        CompressionCase {
            suffix: 'b',
            output: "zstd-without-long",
            args: &["--use-compress-program", "unzstd"],
            zstd: true,
        },
        CompressionCase {
            suffix: 'c',
            output: "zstd",
            args: &["--use-compress-program", "unzstd", "--long=30"],
            zstd: true,
        },
    ];
    for (index, case) in cases.iter().enumerate() {
        assert_compression_action(&binary, &alias, &temp, index, case)?;
    }

    cleanup(&temp);
    Ok(())
}

struct CompressionCase {
    suffix: char,
    output: &'static str,
    args: &'static [&'static str],
    zstd: bool,
}

fn assert_compression_action(
    binary: &Path,
    alias: &Path,
    runner_temp: &Path,
    index: usize,
    case: &CompressionCase,
) -> Result<(), Box<dyn Error>> {
    let key = format!(
        "mise-tools-v2-payload-v3-{}",
        case.suffix.to_string().repeat(64)
    );
    let stage_output = runner_temp.join(format!("stage-{index}.out"));
    let stage_path = request_stage_path(binary, runner_temp, &stage_output, &key)?;
    let member = stage_path
        .strip_prefix(runner_temp)?
        .to_str()
        .ok_or("non-UTF-8 stage member")?
        .to_owned();
    let raw_path = runner_temp.join(format!("raw-{index}.tar"));
    let archive_path = runner_temp.join(format!("archive-{index}.tar"));
    let payload = b"cache tar adapter integration payload";
    fs::write(&raw_path, ustar_member(&member, payload)?)?;
    let zstd_path = if case.zstd {
        write_fake_zstd(runner_temp, index, &raw_path)?
    } else {
        gzip_archive(&raw_path, &archive_path)?;
        PathBuf::new()
    };
    if case.zstd {
        fs::write(&archive_path, b"zstd input consumed by the fixture filter")?;
    }
    let action_output = runner_temp.join(format!("action-{index}.out"));
    create_private_file(&action_output)?;
    let output = invoke_tar_action(
        alias,
        runner_temp,
        &archive_path,
        &action_output,
        &key,
        case.args,
        case.zstd.then_some(zstd_path.as_path()),
    )?;
    assert_eq!(code(&output), 0, "{:?}", output.stderr);

    let lines = fs::read_to_string(action_output)?;
    let mut lines = lines.lines();
    assert!(
        lines
            .next()
            .is_some_and(|line| line.starts_with("cache-stage-claim=v1."))
    );
    assert_eq!(
        lines.next(),
        Some(format!("cache-stage-compression={}", case.output).as_str())
    );
    assert_eq!(lines.next(), None);
    assert_eq!(fs::read(stage_path)?, payload);
    Ok(())
}

fn request_stage_path(
    binary: &Path,
    runner_temp: &Path,
    output: &Path,
    key: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    create_private_file(output)?;
    let mut command = Command::new(binary);
    command.current_dir(runner_temp).env_clear();
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    let result = command
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_RUN_ID", "42")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_OUTPUT", output)
        .env("VELNOR_INTERNAL_OP", "normal-tools-cache-stage-path-v1")
        .env("VELNOR_NORMAL_CACHE_KEY", key)
        .output()?;
    assert_eq!(code(&result), 0, "{:?}", result.stderr);
    let body = fs::read_to_string(output)?;
    let value = body
        .lines()
        .find_map(|line| line.strip_prefix("stage_path="))
        .ok_or("missing stage_path output")?;
    Ok(PathBuf::from(value))
}

fn invoke_tar_action(
    alias: &Path,
    runner_temp: &Path,
    archive: &Path,
    output: &Path,
    key: &str,
    compression: &[&str],
    zstd_path: Option<&Path>,
) -> Result<Output, Box<dyn Error>> {
    let mut command = Command::new(alias);
    command
        .current_dir(runner_temp)
        .args(["-xf"])
        .arg(archive)
        .arg("-P")
        .arg("-C")
        .arg(runner_temp)
        .env_clear();
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    command
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_RUN_ID", "42")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_WORKSPACE", runner_temp)
        .env("GITHUB_OUTPUT", output)
        .env("VELNOR_INTERNAL_OP", OP)
        .env("VELNOR_CACHE_STAGE_NAMESPACE", "normal-tools-v3")
        .env("VELNOR_CACHE_STAGE_KEY", key)
        .env("VELNOR_CACHE_STAGE_ACCESS", "new");
    command.args(compression);
    if let Some(path) = zstd_path {
        command.env("VELNOR_CACHE_ZSTD_PATH", path);
    }
    Ok(command.output()?)
}

fn gzip_archive(raw: &Path, archive: &Path) -> Result<(), Box<dyn Error>> {
    let status = Command::new("gzip")
        .arg("-c")
        .stdin(std::process::Stdio::from(std::fs::File::open(raw)?))
        .stdout(std::process::Stdio::from(std::fs::File::create(archive)?))
        .status()?;
    if !status.success() {
        return Err("gzip fixture generation failed".into());
    }
    Ok(())
}

fn write_fake_zstd(
    runner_temp: &Path,
    index: usize,
    raw_archive: &Path,
) -> Result<PathBuf, Box<dyn Error>> {
    let script = runner_temp.join(format!("zstd-fixture-{index}"));
    let quoted = shell_quote(raw_archive.to_str().ok_or("non-UTF-8 fixture path")?);
    fs::write(
        &script,
        format!("#!/bin/sh\n/bin/cat >/dev/null\n/bin/cat {quoted}\n"),
    )?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700))?;
    Ok(script)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn create_private_file(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    Ok(())
}

fn ustar_member(path: &str, payload: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    const BLOCK: usize = 512;
    let path_bytes = path.as_bytes();
    if path_bytes.len() > 100 {
        return Err("test member path exceeds USTAR name field".into());
    }
    let mut header = [0_u8; BLOCK];
    header[..path_bytes.len()].copy_from_slice(path_bytes);
    write_octal(&mut header[100..108], 0o600);
    write_octal(&mut header[108..116], 0);
    write_octal(&mut header[116..124], 0);
    write_octal(&mut header[124..136], u64::try_from(payload.len())?);
    write_octal(&mut header[136..148], 0);
    header[148..156].fill(b' ');
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|byte| u64::from(*byte)).sum::<u64>();
    let checksum = format!("{checksum:06o}");
    header[148..154].copy_from_slice(checksum.as_bytes());
    header[154] = 0;
    header[155] = b' ';
    let mut archive = header.to_vec();
    archive.extend_from_slice(payload);
    archive.resize(archive.len().div_ceil(BLOCK) * BLOCK, 0);
    archive.extend_from_slice(&[0; BLOCK * 2]);
    Ok(archive)
}

fn write_octal(field: &mut [u8], value: u64) {
    let digit_count = field.len() - 1;
    let value = format!("{value:0digit_count$o}");
    field[..digit_count].copy_from_slice(value.as_bytes());
    field[digit_count] = 0;
}
