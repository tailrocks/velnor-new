use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};

const HELPER_PROCESS_DEADLINE: Duration = Duration::from_secs(90);
pub(super) const MAX_CAPTURED_OUTPUT: usize = 16 * 1024;

#[derive(Debug)]
pub(super) struct HelperOutput {
    pub(super) success: bool,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
}

type InvalidStateRoot = (&'static str, String, PathBuf);

pub(super) async fn verify_persistent_state(
    helper: &Path,
    install_directory: &Path,
    state_directory: &Path,
    request: &Value,
) -> Result<(), Box<dyn Error>> {
    let accepted = invoke_helper(helper, request).await?;
    assert!(accepted.success, "valid historical bundle was rejected");
    assert_eq!(accepted.stdout, br#"{"schema":2,"result":"verified"}"#);
    assert!(accepted.stderr.len() <= MAX_CAPTURED_OUTPUT);
    let cache_directory = state_directory.join("attestation-tuf-cache");
    assert_private_directory(&cache_directory)?;
    assert!(!install_directory.join("attestation-tuf-cache").exists());
    let reused = invoke_helper(helper, request).await?;
    assert!(
        reused.success,
        "persistent TUF state failed across processes"
    );
    assert_private_directory(&cache_directory)?;
    Ok(())
}

pub(super) async fn reject_invalid_state_roots(
    helper: &Path,
    parent: &Path,
    request: &Value,
) -> Result<(), Box<dyn Error>> {
    for (label, invalid_root, cache_root) in invalid_state_roots(parent)? {
        let mut invalid = request.clone();
        invalid["state_directory"] = json!(invalid_root);
        let result = invoke_helper(helper, &invalid).await?;
        assert!(!result.success, "{label} state root was accepted");
        assert!(result.stdout.is_empty(), "{label} returned success data");
        assert!(!cache_root.exists(), "{label} state root was touched");
    }
    Ok(())
}

pub(super) async fn reject_invalid_requests(
    helper: &Path,
    private_directory: &Path,
    request: &Value,
) -> Result<(), Box<dyn Error>> {
    for (index, (label, mut mutated)) in invalid_metadata_requests(request).into_iter().enumerate()
    {
        let rejected_state = private_directory.join(format!("rejected-state-{index}"));
        fs::create_dir(&rejected_state)?;
        set_private_directory(&rejected_state)?;
        if label != "missing state directory" {
            mutated["state_directory"] =
                json!(rejected_state.to_str().ok_or("non-UTF-8 test path")?);
        }
        let result = invoke_helper(helper, &mutated).await?;
        assert!(!result.success, "{label} mutation was accepted");
        assert!(result.stdout.is_empty(), "{label} returned success data");
        assert!(result.stderr.len() <= MAX_CAPTURED_OUTPUT);
        assert!(!rejected_state.join("attestation-tuf-cache").exists());
    }
    for (label, mutated) in invalid_claim_requests(request) {
        let result = invoke_helper(helper, &mutated).await?;
        assert!(!result.success, "{label} mutation was accepted");
        assert!(result.stdout.is_empty(), "{label} returned success data");
        assert!(result.stderr.len() <= MAX_CAPTURED_OUTPUT);
    }
    Ok(())
}

fn invalid_claim_requests(base: &Value) -> Vec<(&'static str, Value)> {
    vec![
        mutate(base, "signer", |request| {
            request["expected"]["signer"] = json!("https://example.invalid/signer");
        }),
        mutate(base, "signer commit ID", |request| {
            request["expected"]["signer_digest"] = json!("0".repeat(40));
        }),
        mutate(base, "source repository", |request| {
            request["expected"]["source"] = json!("https://example.invalid/repo");
        }),
        mutate(base, "source commit ID", |request| {
            request["expected"]["source_digest"] = json!("0".repeat(40));
        }),
        mutate(base, "source ref", |request| {
            request["expected"]["source_ref"] = json!("refs/heads/other");
        }),
        mutate(base, "build config", |request| {
            request["expected"]["build_config"] = json!("https://example.invalid/build.yml");
        }),
        mutate(base, "build config commit ID", |request| {
            request["expected"]["build_config_digest"] = json!("0".repeat(40));
        }),
        mutate(base, "checksum subject", |request| {
            request["checksum_subject"]["digest"] = json!("0".repeat(64));
        }),
        mutate(base, "target subject", |request| {
            request["target_subject"]["digest"] = json!("0".repeat(64));
        }),
    ]
}

fn invalid_metadata_requests(base: &Value) -> Vec<(&'static str, Value)> {
    vec![
        mutate(base, "legacy request schema", |request| {
            request["schema"] = json!(1);
        }),
        mutate(base, "missing state directory", |request| {
            if let Some(object) = request.as_object_mut() {
                object.remove("state_directory");
            }
        }),
        mutate(base, "unsupported caller cache path", |request| {
            request["cache_path"] = json!("/tmp/untrusted-cache");
        }),
    ]
}

fn mutate(
    base: &Value,
    label: &'static str,
    change: impl FnOnce(&mut Value),
) -> (&'static str, Value) {
    let mut request = base.clone();
    change(&mut request);
    (label, request)
}

#[cfg(unix)]
fn invalid_state_roots(parent: &Path) -> Result<Vec<InvalidStateRoot>, Box<dyn Error>> {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let traversal = parent.join("traversal").join("..").join("traversal-state");
    let relative = format!("relative-velnor-service-state-{}", std::process::id());
    let unsafe_root = parent.join("shared-state");
    fs::create_dir(&unsafe_root)?;
    fs::set_permissions(&unsafe_root, fs::Permissions::from_mode(0o777))?;
    let symlink_target = parent.join("symlink-target");
    fs::create_dir(&symlink_target)?;
    fs::set_permissions(&symlink_target, fs::Permissions::from_mode(0o700))?;
    let alias = parent.join("state-alias");
    symlink(&symlink_target, &alias)?;
    let oversized = format!("/{}", "s".repeat(4096));
    Ok(vec![
        (
            "relative",
            relative.clone(),
            PathBuf::from(relative).join("attestation-tuf-cache"),
        ),
        (
            "parent traversal",
            traversal.to_string_lossy().into_owned(),
            parent.join("traversal-state/attestation-tuf-cache"),
        ),
        (
            "group/world writable",
            unsafe_root.to_string_lossy().into_owned(),
            unsafe_root.join("attestation-tuf-cache"),
        ),
        (
            "symlink component",
            alias.to_string_lossy().into_owned(),
            symlink_target.join("attestation-tuf-cache"),
        ),
        (
            "oversized",
            oversized,
            parent.join("oversized-state/attestation-tuf-cache"),
        ),
    ])
}

#[cfg(not(unix))]
fn invalid_state_roots(_parent: &Path) -> Result<Vec<InvalidStateRoot>, Box<dyn Error>> {
    Ok(Vec::new())
}

pub(super) async fn invoke_helper(
    helper: &Path,
    request: &Value,
) -> Result<HelperOutput, Box<dyn Error>> {
    let deadline = Instant::now() + HELPER_PROCESS_DEADLINE;
    let input = serde_json::to_vec(request)?;
    let mut child = Command::new(helper)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    write_request(&mut child, &input, deadline).await?;
    let stdout = spawn_bounded_read(child.stdout.take(), "stdout")?;
    let stderr = spawn_bounded_read(child.stderr.take(), "stderr")?;
    let status = wait_bounded(&mut child, deadline).await?;
    let stdout = join_bounded_read(stdout).await?;
    let stderr = join_bounded_read(stderr).await?;
    if stdout.len() > MAX_CAPTURED_OUTPUT || stderr.len() > MAX_CAPTURED_OUTPUT {
        return Err("helper output exceeded protocol limit".into());
    }
    Ok(HelperOutput {
        success: status.success(),
        stdout,
        stderr,
    })
}

async fn write_request(
    child: &mut Child,
    input: &[u8],
    deadline: Instant,
) -> Result<(), Box<dyn Error>> {
    let mut stdin = child.stdin.take().ok_or("helper stdin was not piped")?;
    match timeout_at(deadline, stdin.write_all(input)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => {
            kill_and_reap(child).await?;
            Err(error.into())
        }
        Err(_) => {
            kill_and_reap(child).await?;
            Err("helper request write exceeded its deadline".into())
        }
    }
}

fn spawn_bounded_read(
    stream: Option<impl AsyncRead + Unpin + Send + 'static>,
    name: &'static str,
) -> Result<JoinHandle<io::Result<Vec<u8>>>, Box<dyn Error>> {
    let stream = stream.ok_or_else(|| format!("helper {name} was not piped"))?;
    Ok(tokio::spawn(async move {
        let mut bytes = Vec::new();
        stream
            .take((MAX_CAPTURED_OUTPUT + 1) as u64)
            .read_to_end(&mut bytes)
            .await?;
        Ok(bytes)
    }))
}

async fn wait_bounded(
    child: &mut Child,
    deadline: Instant,
) -> Result<std::process::ExitStatus, Box<dyn Error>> {
    if let Ok(status) = timeout_at(deadline, child.wait()).await {
        Ok(status?)
    } else {
        kill_and_reap(child).await?;
        Err("helper process exceeded its deadline".into())
    }
}

async fn kill_and_reap(child: &mut Child) -> Result<(), Box<dyn Error>> {
    if child.try_wait()?.is_none() {
        child.start_kill()?;
    }
    child.wait().await?;
    Ok(())
}

async fn join_bounded_read(
    task: JoinHandle<io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(task.await??)
}

#[cfg(unix)]
pub(super) fn assert_private_directory(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata = fs::symlink_metadata(path)?;
    assert!(metadata.is_dir());
    assert!(!metadata.file_type().is_symlink());
    assert_eq!(metadata.uid(), rustix::process::geteuid().as_raw());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    Ok(())
}

#[cfg(unix)]
pub(super) fn set_private_directory(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn set_private_directory(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn assert_private_executable(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::symlink_metadata(path)?;
    assert!(metadata.is_file());
    assert!(!metadata.file_type().is_symlink());
    assert_ne!(metadata.permissions().mode() & 0o111, 0);
    assert_eq!(metadata.permissions().mode() & 0o022, 0);
    Ok(())
}

#[cfg(unix)]
pub(super) fn set_read_only_install(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o555))?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn set_read_only_install(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn set_read_only_executable(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o555))?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn set_read_only_executable(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn assert_read_only_install(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata = fs::symlink_metadata(path)?;
    assert!(metadata.is_dir());
    assert!(!metadata.file_type().is_symlink());
    assert_eq!(metadata.uid(), rustix::process::geteuid().as_raw());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o555);
    Ok(())
}

#[cfg(unix)]
pub(super) fn restore_install_writability(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn restore_install_writability(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn assert_read_only_install(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn assert_private_executable(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn assert_private_directory(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}
