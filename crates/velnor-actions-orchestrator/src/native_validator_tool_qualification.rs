// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write, copy};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};
use velnor_actions_mise::IsolatedCommand;

#[path = "native_validator_tool_qualification/receipts.rs"]
mod receipts;

use crate::OrchestratorError;
use crate::internal::internal;
use crate::safe_read::read_event_file;

pub const NATIVE_VALIDATOR_TOOL_QUALIFICATION_OP: &str = "qualify-validator-tools-v1";

const EVENT_ENV: &str = "GITHUB_EVENT_NAME";
const HEAD_ENV: &str = "GITHUB_SHA";
const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";
const TARGET_DIR_ENV: &str = "CARGO_TARGET_DIR";
const SOURCE_BINARY_RELATIVE_PATH: &str = "release/velnor-actions";
const EVIDENCE_DIR: &str = "source-qualification-artifacts";
const PHASE_DIR: &str = "native-validator-tool-qualification";
const PLAN_FILE: &str = "validator-tool-install-plan.json";
const ADMISSION_FILE: &str = "validator-tool-lock-admission.txt";
const RECIPE_FILE: &str = "scripts/generator-release/source-qualification-validator-recipes.json";
const CURL: &str = "/usr/bin/curl";
const MAX_RECEIPT_BYTES: u64 = 1024 * 1024;
const PROCESS_OUTPUT_BYTES: usize = 64 * 1024;
const ARCHIVE_TIMEOUT: Duration = Duration::from_secs(300);

struct Context {
    workspace: PathBuf,
    runner_temp: PathBuf,
    target_dir: PathBuf,
    binary_receipt_path: String,
    event_name: String,
    source_head: String,
    current_executable: PathBuf,
    binary_version: String,
}

struct Archive {
    url: String,
    checksum: String,
    path: PathBuf,
}

impl Context {
    fn from_environment(
        runner_temp: PathBuf,
        target_dir: PathBuf,
        binary_version: &str,
    ) -> Result<Self, OrchestratorError> {
        let workspace = std::env::current_dir()
            .map_err(|error| OrchestratorError::io("current_dir", error.to_string()))?;
        let event_name = std::env::var(EVENT_ENV).unwrap_or_default();
        let source_head = std::env::var(HEAD_ENV).unwrap_or_else(|_| "unavailable".to_owned());
        let binary_receipt_path = std::env::var(TARGET_DIR_ENV)
            .map(|path| format!("{path}/{SOURCE_BINARY_RELATIVE_PATH}"))
            .map_err(|_| internal("native_validator_source_cli_path_encoding"))?;
        let current_executable = std::env::current_exe()
            .map_err(|error| OrchestratorError::io("current_exe", error.to_string()))?;
        Ok(Self {
            workspace,
            runner_temp,
            target_dir,
            binary_receipt_path,
            event_name,
            source_head,
            current_executable,
            binary_version: binary_version.to_owned(),
        })
    }
}

pub fn run_native_validator_tool_qualification_from_env(
    binary_version: &str,
) -> Result<(), OrchestratorError> {
    let event_name = std::env::var(EVENT_ENV).unwrap_or_default();
    if event_name != "workflow_dispatch" {
        return Err(internal("native_validator_manual_dispatch_required"));
    }
    let runner_temp = absolute_path_env(RUNNER_TEMP_ENV)?;
    let target_dir = absolute_path_env(TARGET_DIR_ENV)?;
    let evidence = runner_temp.join(EVIDENCE_DIR);
    require_directory(&evidence)?;
    require_directory(&evidence.join("logs"))?;
    let phase = evidence.join(PHASE_DIR);
    create_private_directory(&phase)?;
    let source_head = std::env::var(HEAD_ENV).unwrap_or_else(|_| "unavailable".to_owned());
    let log_path = phase.join("qualification.log");
    let operation = (|| {
        let context = Context::from_environment(runner_temp, target_dir, binary_version)?;
        run_owned_phase(&context, &phase, &log_path)
    })();
    let operation = match operation {
        Ok(()) => emit_log(&log_path, &mut std::io::stdout().lock()),
        Err(error) => match emit_log(&log_path, &mut std::io::stderr().lock()) {
            Ok(()) => Err(error),
            Err(log_error) => Err(log_error),
        },
    };
    let status = write_status(&phase.join("status.txt"), &source_head, &event_name, operation.is_ok());
    match (operation, status) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), _) | (Ok(()), Err(error)) => Err(error),
    }
}

fn run_owned_phase(
    context: &Context,
    phase: &Path,
    log_path: &Path,
) -> Result<(), OrchestratorError> {
    let evidence = context.runner_temp.join(EVIDENCE_DIR);
    let mut log = create_private_file(log_path)?;
    validate_cli_identity(context, &evidence)?;
    let archives = receipts::admitted_archives(context, &evidence, phase)?;
    let archive_root = phase.join("archives");
    create_private_directory(&archive_root)?;
    for archive in &archives {
        download_archive(archive, &mut log)?;
    }
    receipts::write_request(context, phase, &archives)?;
    let request = phase.join("request.json");
    let work_dir = phase.join("work");
    let candidate = phase.join("candidate.json");
    if let Err(error) = crate::qualification_candidate::qualify_tools(&request, &work_dir, &candidate)
    {
        append_log(&mut log, error.to_string().as_bytes())?;
        append_log(&mut log, b"\n")?;
        return Err(error);
    }
    append_log(
        &mut log,
        b"Qualified-tool authoring candidate written; independent CI verification required.\n",
    )?;
    log.flush().map_err(|error| io_error(log_path, error))
}

fn validate_cli_identity(context: &Context, evidence: &Path) -> Result<(), OrchestratorError> {
    if context.event_name != "workflow_dispatch"
        || !velnor_actions_contract::ids::is_lower_hex_len(&context.source_head, 40)
    {
        return Err(internal("native_validator_source_identity"));
    }
    let binary = context.target_dir.join(SOURCE_BINARY_RELATIVE_PATH);
    let metadata = fs::symlink_metadata(&binary).map_err(|error| io_error(&binary, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || !is_executable(&metadata) {
        return Err(internal("native_validator_source_cli_unavailable"));
    }
    let canonical_binary = binary
        .canonicalize()
        .map_err(|error| io_error(&binary, error))?;
    let canonical_executable = context
        .current_executable
        .canonicalize()
        .map_err(|error| io_error(&context.current_executable, error))?;
    if canonical_binary != canonical_executable {
        return Err(internal("native_validator_source_cli_path_mismatch"));
    }
    let binary_sha256 = hash_regular_file(&binary)?;
    let identity = read_runner_text(&evidence.join("cli-identity.txt"), "native_validator_cli_identity")?;
    if !contains_line(&identity, &format!("source_head={}", context.source_head))
        || !contains_line(
            &identity,
            &format!("binary_path={}", context.binary_receipt_path),
        )
        || !contains_line(&identity, &format!("expected_version={}", context.binary_version))
        || !contains_line(&identity, &format!("binary_version={}", context.binary_version))
        || !contains_line(&identity, &format!("binary_sha256={binary_sha256}"))
    {
        return Err(internal("native_validator_source_cli_identity_mismatch"));
    }
    Ok(())
}

fn download_archive(archive: &Archive, log: &mut File) -> Result<(), OrchestratorError> {
    drop(create_private_file(&archive.path)?);
    let args: Vec<OsString> = [
        "--fail",
        "--location",
        "--silent",
        "--show-error",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--connect-timeout",
        "15",
        "--max-time",
        "300",
        "--retry",
        "3",
        "--retry-all-errors",
        "--retry-delay",
        "1",
        "--output",
    ]
    .into_iter()
    .map(OsString::from)
    .chain([
        archive.path.as_os_str().to_owned(),
        OsString::from(archive.url.as_str()),
    ])
    .collect();
    let command = IsolatedCommand::repo_task(CURL, args, &[])
        .map_err(|error| internal(&format!("native_validator_curl_command:{error}")))?;
    let output = command.run_bounded(PROCESS_OUTPUT_BYTES, ARCHIVE_TIMEOUT);
    match output {
        Ok(output) => {
            append_log(log, &output.stdout)?;
            append_log(log, &output.stderr)?;
            if !output.success {
                return Err(internal("native_validator_archive_download_failed"));
            }
        }
        Err(error) => {
            append_log(log, error.to_string().as_bytes())?;
            append_log(log, b"\n")?;
            return Err(internal("native_validator_archive_download_failed"));
        }
    }
    let metadata = fs::symlink_metadata(&archive.path)
        .map_err(|error| io_error(&archive.path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(internal("native_validator_archive_download_unsafe"));
    }
    Ok(())
}

fn absolute_path_env(name: &str) -> Result<PathBuf, OrchestratorError> {
    let path = std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| internal(&format!("native_validator_path_invalid:{name}")))?;
    Ok(path)
}

fn require_directory(path: &Path) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(internal("native_validator_evidence_path_unavailable"));
    }
    Ok(())
}

fn create_private_directory(path: &Path) -> Result<(), OrchestratorError> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|error| io_error(path, error))
}

fn create_private_file(path: &Path) -> Result<File, OrchestratorError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|error| io_error(path, error))
}

fn write_status(
    path: &Path,
    source_head: &str,
    event_name: &str,
    success: bool,
) -> Result<(), OrchestratorError> {
    let result = if success { "success" } else { "failed" };
    let exit_status = i32::from(!success);
    let status = format!(
        "source_head={source_head}\nevent={event_name}\nresult={result}\nexit_status={exit_status}\n"
    );
    let mut file = create_private_file(path)?;
    file.write_all(status.as_bytes())
        .map_err(|error| io_error(path, error))
}

fn read_runner_text(path: &Path, problem: &str) -> Result<String, OrchestratorError> {
    read_event_file(path, MAX_RECEIPT_BYTES).map_err(|_| internal(problem))
}

fn contains_line(text: &str, expected: &str) -> bool {
    text.lines().any(|line| line == expected)
}

fn append_log(log: &mut File, bytes: &[u8]) -> Result<(), OrchestratorError> {
    log.write_all(bytes)
        .map_err(|error| OrchestratorError::io("qualification.log", error.to_string()))
}

fn emit_log(path: &Path, output: &mut dyn Write) -> Result<(), OrchestratorError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(internal("native_validator_log_unsafe"));
        }
        Err(error) => return Err(io_error(path, error)),
        Ok(_) => {}
    }
    let mut file = File::open(path).map_err(|error| io_error(path, error))?;
    copy(&mut file, output).map_err(|error| io_error(path, error))?;
    output
        .flush()
        .map_err(|error| OrchestratorError::io("qualification_output", error.to_string()))
}

fn hash_regular_file(path: &Path) -> Result<String, OrchestratorError> {
    let descriptor = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    let file_type = rustix::fs::fstat(&descriptor)
        .map(|stat| rustix::fs::FileType::from_raw_mode(stat.st_mode))
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    if file_type != rustix::fs::FileType::RegularFile {
        return Err(internal("native_validator_source_cli_not_regular"));
    }
    let mut file = File::from(descriptor);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| io_error(path, error))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex_lower(&hasher.finalize()))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn sha256(bytes: &[u8]) -> String {
    hex_lower(&Sha256::digest(bytes))
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
const fn is_executable(_: &fs::Metadata) -> bool {
    false
}

fn io_error(path: &Path, error: impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}
