//! Private MBX store lifecycle regressions.

use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{EXPORT_SCRIPT, IMPORT_SCRIPT, STORE_INIT_SCRIPT};

static NEXT_SANDBOX: AtomicUsize = AtomicUsize::new(0);

const RUN_ID: &str = "37178675286";
const ATTEMPT: &str = "2";
const JOB_ID: &str = "rust_app";

struct Sandbox(PathBuf);

impl Sandbox {
    fn create() -> io::Result<Self> {
        let id = NEXT_SANDBOX.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-mbx-store-tests-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}

fn run_script(
    script: &str,
    environment: &[(&str, &str)],
    path_prefix: Option<&Path>,
) -> io::Result<Output> {
    let mut command = Command::new("bash");
    command.args(["-c", script]);
    for (key, value) in environment {
        command.env(key, value);
    }
    if let Some(prefix) = path_prefix {
        let path = format!(
            "{}:{}",
            prefix.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        command.env("PATH", path);
    }
    command.output()
}

fn init_root(sandbox: &Sandbox, attempt: &str) -> io::Result<PathBuf> {
    let env_file = sandbox.path().join(format!("github-env-{attempt}"));
    let temp = sandbox.path().to_string_lossy().into_owned();
    let env_path = env_file.to_string_lossy().into_owned();
    let output = run_script(
        STORE_INIT_SCRIPT,
        &[
            ("RUNNER_TEMP", &temp),
            ("GITHUB_ENV", &env_path),
            ("GITHUB_RUN_ID", RUN_ID),
            ("GITHUB_RUN_ATTEMPT", attempt),
            ("GITHUB_JOB", JOB_ID),
        ],
        None,
    )?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }
    let environment = fs::read_to_string(env_file)?;
    let root = environment
        .lines()
        .find_map(|line| line.strip_prefix("MBX_CACHE_DIR="))
        .ok_or_else(|| io::Error::other("MBX_CACHE_DIR missing from GITHUB_ENV"))?;
    Ok(PathBuf::from(root))
}

fn fake_mbx(sandbox: &Sandbox) -> io::Result<PathBuf> {
    let bin = sandbox.path().join("bin");
    fs::create_dir_all(&bin)?;
    let command = bin.join("mbx");
    fs::write(
        &command,
        r##"#!/bin/bash
set -u
if [ "$1" = "gc" ]; then
    if [ "${MBX_FAKE_MODE:-}" = "gc-fail" ]; then echo "gc failed" >&2; exit 1; fi
    exit 0
fi
if [ "$1" = "cache" ] && [ "$2" = "dir" ]; then
    if [ "${MBX_FAKE_MODE:-}" = "wrong-store" ]; then
        printf '%s\n' "$RUNNER_TEMP/foreign-mbx/actions"
    else
        printf '%s\n' "$MBX_CACHE_DIR/actions"
    fi
    exit 0
fi
if [ "$1" = "cache" ] && [ "$2" = "import" ]; then
    if [ "${MBX_FAKE_MODE:-}" = "import-fail" ]; then echo "import failed" >&2; exit 1; fi
    if [ -f "$3/payload" ]; then echo "imported"; exit 0; fi
    echo "bundle payload missing" >&2
    exit 1
fi
if [ "$1" = "cache" ] && [ "$2" = "export" ]; then
    if [ "${MBX_FAKE_MODE:-}" = "export-fail" ]; then echo "export failed" >&2; exit 1; fi
    if [ "${MBX_FAKE_MODE:-}" = "no-entry" ]; then
        echo "no completed mbx builds are recorded for export group" >&2
        exit 1
    fi
    for target do :; done
    mkdir -p "$target"
    printf 'bundle payload\n' > "$target/payload"
    if [ "${MBX_FAKE_MODE:-}" = "partial-export" ]; then exit 1; fi
    exit 0
fi
echo "unexpected mbx argv: $*" >&2
exit 2
"##,
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&command, fs::Permissions::from_mode(0o700))?;
    }
    Ok(bin)
}

fn export_result(
    sandbox: &Sandbox,
    root: &Path,
    mode: &str,
) -> io::Result<(Output, String, String)> {
    export_with_cache_root(sandbox, root, mode)
}

fn import_result(
    sandbox: &Sandbox,
    root: &Path,
    matched: &str,
    mode: &str,
) -> io::Result<(Output, String, String, String)> {
    import_result_with_handoff_failure(sandbox, root, matched, mode, None)
}

fn import_result_with_handoff_failure(
    sandbox: &Sandbox,
    root: &Path,
    matched: &str,
    mode: &str,
    failed_handoff: Option<&str>,
) -> io::Result<(Output, String, String, String)> {
    let bin = fake_mbx(sandbox)?;
    let temp = sandbox.path().to_string_lossy().into_owned();
    let cache = root.to_string_lossy().into_owned();
    let output_path = sandbox.path().join("import-github-output");
    let summary_path = sandbox.path().join("import-github-summary");
    let env_path = sandbox.path().join("import-github-env");
    if failed_handoff == Some("output") {
        fs::create_dir(&output_path)?;
    } else {
        fs::write(&output_path, "")?;
    }
    fs::write(&summary_path, "")?;
    if failed_handoff == Some("env") {
        fs::create_dir(&env_path)?;
    } else {
        fs::write(&env_path, "")?;
    }
    let output_arg = output_path.to_string_lossy().into_owned();
    let summary_arg = summary_path.to_string_lossy().into_owned();
    let env_arg = env_path.to_string_lossy().into_owned();
    let output = run_script(
        IMPORT_SCRIPT,
        &[
            ("RUNNER_TEMP", &temp),
            ("MBX_CACHE_DIR", &cache),
            ("MATCHED", matched),
            ("GITHUB_OUTPUT", &output_arg),
            ("GITHUB_STEP_SUMMARY", &summary_arg),
            ("GITHUB_ENV", &env_arg),
            ("MBX_FAKE_MODE", mode),
        ],
        Some(&bin),
    )?;
    let outputs = if output_path.is_file() {
        fs::read_to_string(&output_path)?
    } else {
        String::new()
    };
    let summary = fs::read_to_string(summary_path)?;
    let github_env = if env_path.is_file() {
        fs::read_to_string(&env_path)?
    } else {
        String::new()
    };
    Ok((output, outputs, summary, github_env))
}

fn export_with_cache_root(
    sandbox: &Sandbox,
    cache_root: &Path,
    mode: &str,
) -> io::Result<(Output, String, String)> {
    let bin = fake_mbx(sandbox)?;
    let output_path = sandbox.path().join("github-output");
    let summary_path = sandbox.path().join("github-summary");
    fs::write(&output_path, "")?;
    fs::write(&summary_path, "")?;
    let temp = sandbox.path().to_string_lossy().into_owned();
    let cache = cache_root.to_string_lossy().into_owned();
    let output_path = output_path.to_string_lossy().into_owned();
    let summary_path = summary_path.to_string_lossy().into_owned();
    let mut environment = vec![
        ("RUNNER_TEMP", &temp),
        ("MBX_CACHE_DIR", &cache),
        ("MBX_CACHE_EXPORT_GROUP", "g-run-2-job"),
        ("GITHUB_RUN_ID", RUN_ID),
        ("GITHUB_RUN_ATTEMPT", ATTEMPT),
        ("GITHUB_JOB", JOB_ID),
        ("GITHUB_OUTPUT", &output_path),
        ("GITHUB_STEP_SUMMARY", &summary_path),
        ("MBX_FAKE_MODE", mode),
    ];
    if mode == "import-uncertain" {
        environment.push(("MBX_CACHE_IMPORT_STATE", "unavailable"));
    } else if mode == "imported" {
        environment.push(("MBX_CACHE_IMPORT_STATE", "imported"));
    } else {
        environment.push(("MBX_CACHE_IMPORT_STATE", "cold"));
    }
    let output = run_script(EXPORT_SCRIPT, &environment, Some(&bin))?;
    let outputs = fs::read_to_string(output_path)?;
    let summary = fs::read_to_string(summary_path)?;
    Ok((output, outputs, summary))
}

fn init_store(sandbox: &Sandbox, attempt: &str) -> io::Result<PathBuf> {
    let root = init_root(sandbox, attempt)?;
    fs::create_dir(root.join("actions"))?;
    fs::write(root.join("actions/sentinel"), "store stays owned\n")?;
    Ok(root)
}

fn assert_unavailable(output: Output, outputs: &str, summary: &str) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(outputs.contains("ready=false"), "{outputs}");
    assert!(
        outputs.contains("acceptance=cache_unavailable"),
        "{outputs}"
    );
    assert!(!outputs.contains("ready=true"), "{outputs}");
    assert!(summary.contains("cache_unavailable"), "{summary}");
}

#[test]
fn init_creates_distinct_private_roots_with_exact_owner_identity() -> Result<(), Box<dyn Error>> {
    let sandbox = Sandbox::create()?;
    let first = init_root(&sandbox, ATTEMPT)?;
    let second = init_root(&sandbox, ATTEMPT)?;
    let canonical_temp = fs::canonicalize(sandbox.path())?;
    assert_ne!(first, second);
    assert_eq!(first.parent(), Some(canonical_temp.as_path()));
    assert_eq!(second.parent(), Some(canonical_temp.as_path()));
    let marker = fs::read_to_string(first.join(".velnor-mbx-owner"))?;
    assert_eq!(
        marker,
        format!("run_id={RUN_ID}\njob={JOB_ID}\nattempt={ATTEMPT}\n")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&first)?.permissions().mode() & 0o777, 0o700);
    }
    Ok(())
}

#[path = "mbx_store_restore_tests.rs"]
mod restore_tests;

#[path = "mbx_store_failure_tests.rs"]
mod failure_tests;

#[path = "mbx_store_export_tests.rs"]
mod export_tests;
