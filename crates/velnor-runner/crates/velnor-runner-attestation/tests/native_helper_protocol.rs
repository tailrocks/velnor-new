//! Process-level verification against the checked-in historical Sigstore bundle.

use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};

const HELPER_PROCESS_DEADLINE: Duration = Duration::from_secs(90);
const MAX_CAPTURED_OUTPUT: usize = 16 * 1024;
const HISTORICAL_SOURCE: &str = "4f6def90e7b1008626db18675d1cac129b8f2ad7";
const HISTORICAL_WORKFLOW: &str =
    "https://github.com/tailrocks/velnor-new/.github/workflows/image-release.yml@refs/heads/main";
const HISTORICAL_SIGNER: &str =
    "https://github.com/tailrocks/velnor-new/.github/workflows/image-release.yml@refs/heads/main";

#[derive(Debug)]
struct HelperOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[tokio::test]
async fn compiled_helper_verifies_real_bundle_and_rejects_claim_mutations()
-> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let private_directory = directory.path().canonicalize()?;
    let helper = private_directory.join("velnor-runner-attestation-helper");
    fs::copy(
        env!("CARGO_BIN_EXE_velnor-runner-attestation-helper"),
        &helper,
    )?;
    assert_private_directory(&private_directory)?;
    assert_private_executable(&helper)?;

    let request = historical_request()?;
    if directory.path() != private_directory {
        let aliased_helper = directory.path().join("velnor-runner-attestation-helper");
        let rejected = invoke_helper(&aliased_helper, &request).await?;
        assert!(
            !rejected.success,
            "helper accepted a cache path with a symlink ancestor"
        );
        assert!(rejected.stdout.is_empty());
    }
    let accepted = invoke_helper(&helper, &request).await?;
    assert!(accepted.success, "valid historical bundle was rejected");
    assert_eq!(accepted.stdout, br#"{"schema":1,"result":"verified"}"#);
    assert!(accepted.stderr.len() <= MAX_CAPTURED_OUTPUT);
    assert_private_directory(&private_directory.join("attestation-tuf-cache"))?;

    for (label, mutated) in invalid_requests(&request) {
        let result = invoke_helper(&helper, &mutated).await?;
        assert!(!result.success, "{label} mutation was accepted");
        assert!(result.stdout.is_empty(), "{label} returned success data");
        assert!(
            result.stderr.len() <= MAX_CAPTURED_OUTPUT,
            "{label} exceeded output cap"
        );
    }
    Ok(())
}

fn historical_request() -> Result<Value, Box<dyn Error>> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let bundle = fs::read(fixtures.join("api-SHA256SUMS-attestation-1.bundle.json"))?;
    let checksums = fs::read(fixtures.join("SHA256SUMS"))?;
    Ok(json!({
        "schema": 1,
        "bundle_base64": base64::engine::general_purpose::STANDARD.encode(bundle),
        "checksum_base64": base64::engine::general_purpose::STANDARD.encode(checksums),
        "expected": {
            "signer": HISTORICAL_SIGNER,
            "signer_digest": HISTORICAL_SOURCE,
            "source": "https://github.com/tailrocks/velnor-new",
            "source_digest": HISTORICAL_SOURCE,
            "source_ref": "refs/heads/main",
            "build_config": HISTORICAL_WORKFLOW,
            "build_config_digest": HISTORICAL_SOURCE
        },
        "checksum_subject": {
            "name": "SHA256SUMS",
            "digest": "4fb1c0c54b92e84e68c816d621f5cb83c946db392995cd245fa6d61b0bd1e491"
        },
        "target_subject": {
            "name": "velnor-runner-linux-amd64.tar",
            "digest": "21b54ea99c42b3a932713ee3999064a6f2cedaf992be1c6125bbf92f07f5761b"
        }
    }))
}

fn invalid_requests(base: &Value) -> Vec<(&'static str, Value)> {
    let mut cases = Vec::new();
    cases.push(mutate(base, "signer", |request| {
        request["expected"]["signer"] = json!("https://example.invalid/signer");
    }));
    cases.push(mutate(base, "signer commit ID", |request| {
        request["expected"]["signer_digest"] = json!("0".repeat(40));
    }));
    cases.push(mutate(base, "source repository", |request| {
        request["expected"]["source"] = json!("https://example.invalid/repo");
    }));
    cases.push(mutate(base, "source commit ID", |request| {
        request["expected"]["source_digest"] = json!("0".repeat(40));
    }));
    cases.push(mutate(base, "source ref", |request| {
        request["expected"]["source_ref"] = json!("refs/heads/other");
    }));
    cases.push(mutate(base, "build config", |request| {
        request["expected"]["build_config"] = json!("https://example.invalid/build.yml");
    }));
    cases.push(mutate(base, "build config commit ID", |request| {
        request["expected"]["build_config_digest"] = json!("0".repeat(40));
    }));
    cases.push(mutate(base, "checksum subject", |request| {
        request["checksum_subject"]["digest"] = json!("0".repeat(64));
    }));
    cases.push(mutate(base, "target subject", |request| {
        request["target_subject"]["digest"] = json!("0".repeat(64));
    }));
    cases.push(mutate(base, "caller cache path", |request| {
        request["cache_path"] = json!("/tmp/untrusted-cache");
    }));
    cases
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

async fn invoke_helper(helper: &Path, request: &Value) -> Result<HelperOutput, Box<dyn Error>> {
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
fn assert_private_directory(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let metadata = fs::symlink_metadata(path)?;
    assert!(metadata.is_dir());
    assert!(!metadata.file_type().is_symlink());
    assert_eq!(metadata.uid(), rustix::process::geteuid().as_raw());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    Ok(())
}

#[cfg(unix)]
fn assert_private_executable(path: &Path) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = fs::symlink_metadata(path)?;
    assert!(metadata.is_file());
    assert!(!metadata.file_type().is_symlink());
    assert_ne!(metadata.permissions().mode() & 0o111, 0);
    assert_eq!(metadata.permissions().mode() & 0o022, 0);
    Ok(())
}

#[cfg(not(unix))]
fn assert_private_executable(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}

#[cfg(not(unix))]
fn assert_private_directory(_path: &Path) -> Result<(), Box<dyn Error>> {
    Ok(())
}
