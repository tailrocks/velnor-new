use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use velnor_actions_contract::PullRequestCachePolicy;

use super::key_script;

const BASE_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HEAD_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

struct TestDirectory(PathBuf);

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("failed to remove MBX cache test directory: {error}");
        }
    }
}

type TestResult<T> = Result<T, Box<dyn Error>>;

fn test_directory() -> TestResult<TestDirectory> {
    let path = std::env::temp_dir().join(format!(
        "velnor-mbx-pr-cache-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path)?;
    Ok(TestDirectory(path))
}

fn run_key_script(
    head_repository: &str,
    base_repository: &str,
    fork: &str,
    number: &str,
    head_sha: &str,
) -> TestResult<String> {
    let temp = test_directory()?;
    let bin = temp.0.join("bin");
    fs::create_dir_all(&bin)?;
    let rustc = bin.join("rustc");
    fs::write(
        &rustc,
        "#!/bin/sh\nprintf '%s\\n' 'rustc 1.98.1' 'host: x86_64-unknown-linux-gnu'\n",
    )?;
    fs::set_permissions(&rustc, fs::Permissions::from_mode(0o755))?;
    let output = temp.0.join("output");
    let github_env = temp.0.join("environment");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let result = Command::new("bash")
        .arg("-c")
        .arg(key_script(PullRequestCachePolicy::SameRepositoryScoped))
        .env("PATH", path)
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("RUNNER_TEMP", &temp.0)
        .env("GITHUB_RUN_ID", "200")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_JOB", "rust-test")
        .env("GITHUB_OUTPUT", &output)
        .env("GITHUB_ENV", github_env)
        .env("CACHE_GENERATION", "mbx-1.21.1")
        .env("CACHE_REVISION", BASE_SHA)
        .env("RUST_TOOLCHAIN", "1.98.1")
        .env("CREATE_EXPORT_GROUP", "false")
        .env("MBX_PR_CACHE_POLICY", "same-repository-scoped")
        .env("MBX_EVENT_NAME", "pull_request")
        .env("MBX_REPOSITORY", "tailrocks/velnor-new")
        .env("MBX_HEAD_REPOSITORY", head_repository)
        .env("MBX_BASE_REPOSITORY", base_repository)
        .env("MBX_HEAD_REPOSITORY_FORK", fork)
        .env("MBX_PR_NUMBER", number)
        .env("MBX_PR_HEAD_SHA", head_sha)
        .output()?;
    assert!(
        result.status.success(),
        "cache-key script failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(fs::read_to_string(output)?)
}

#[test]
fn admitted_same_repository_pull_request_uses_its_own_key_namespace() -> TestResult<()> {
    let output = run_key_script(
        "tailrocks/velnor-new",
        "tailrocks/velnor-new",
        "false",
        "42",
        HEAD_SHA,
    )?;
    assert!(output.contains("pr-cache-allowed=true\n"), "{output}");
    assert!(
        output.contains(&format!("same-repository-pr-42-{HEAD_SHA}")),
        "{output}"
    );
    Ok(())
}

#[test]
fn fork_or_malformed_pull_request_identity_stays_on_the_read_only_key() -> TestResult<()> {
    for (head_repository, fork, number, head_sha) in [
        ("outside/velnor-new", "true", "42", HEAD_SHA),
        ("tailrocks/velnor-new", "false", "42", "not-a-sha"),
        ("tailrocks/velnor-new", "false", "0", HEAD_SHA),
    ] {
        let output = run_key_script(
            head_repository,
            "tailrocks/velnor-new",
            fork,
            number,
            head_sha,
        )?;
        assert!(output.contains("pr-cache-allowed=false\n"), "{output}");
        assert!(output.contains(&format!("-{BASE_SHA}\n")), "{output}");
        assert!(!output.contains("same-repository-pr-"), "{output}");
    }
    Ok(())
}
