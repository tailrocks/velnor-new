use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use velnor_actions_contract::StepKind;

use super::super::{QualificationObserverBinding, qualification_observer_steps};

const SCOPE: &str = "qualification-mbx-v1/cancel-pre-save-victim";
const VERSION: &str = "1.21.1";
const GENERATION: &str = "velnor-mbx-1.21.1";
const PRIMARY: &str = "derived-exact-primary";

#[test]
fn observer_import_skips_miss_and_imports_only_exact_hit() -> Result<(), String> {
    let miss = run_import("miss", "false", "")?;
    assert!(miss.status.success(), "{}", miss.stderr);
    assert!(!miss.import_called);
    assert_eq!(
        miss.selected_root.as_deref(),
        Some(miss.initial_root.as_str())
    );

    let hit = run_import("hit", "true", PRIMARY)?;
    assert!(hit.status.success(), "{}", hit.stderr);
    assert!(hit.import_called);
    assert_eq!(
        hit.selected_root.as_deref(),
        Some(hit.initial_root.as_str())
    );

    let bad_hit = run_import("bad-hit", "true", "different-key")?;
    assert!(!bad_hit.status.success());
    assert!(!bad_hit.import_called);
    Ok(())
}

fn run_import(label: &str, cache_hit: &str, matched: &str) -> Result<ImportRun, String> {
    let scratch = Scratch::new(label)?;
    let runner_temp = scratch.0.join("runner-temp");
    let bin = scratch.0.join("bin");
    fs::create_dir_all(runner_temp.join("mbx-single-bundle")).map_err(|error| io_error(&error))?;
    fs::create_dir_all(&bin).map_err(|error| io_error(&error))?;
    executable(&bin.join("df"), "#!/bin/sh\nexit 0\n")?;
    let marker = scratch.0.join("import-called");
    executable(
        &bin.join("mbx"),
        "#!/bin/sh\nprintf called >> \"$IMPORT_MARKER\"\nprintf imported\\n\n",
    )?;
    let output = scratch.0.join("github-output");
    let github_env = scratch.0.join("github-env");
    fs::write(&output, "").map_err(|error| io_error(&error))?;
    fs::write(&github_env, "").map_err(|error| io_error(&error))?;
    let steps = observer_steps()?;
    let StepKind::Shell { run, .. } = &steps[2].kind else {
        return Err("observer importer is not shell".to_owned());
    };
    let script = run
        .get(2)
        .ok_or_else(|| "observer import script missing".to_owned())?;
    let initial_root = runner_temp.join("cache").display().to_string();
    let result = Command::new("/bin/bash")
        .arg("-c")
        .arg(script)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin:/sbin", bin.display()))
        .env("RUNNER_TEMP", &runner_temp)
        .env("GITHUB_OUTPUT", &output)
        .env("GITHUB_ENV", &github_env)
        .env("GITHUB_RUN_ID", "901")
        .env("GITHUB_RUN_ATTEMPT", "2")
        .env("MBX_CACHE_DIR", &initial_root)
        .env("MBX_CACHE_EXPORT_GROUP", "observer-group")
        .env("IMPORT_MARKER", &marker)
        .env("MATCHED", matched)
        .env("CACHE_HIT", cache_hit)
        .env("EXPECTED_KEY", PRIMARY)
        .output()
        .map_err(|error| io_error(&error))?;
    let selected_root = fs::read_to_string(output)
        .map_err(|error| io_error(&error))?
        .lines()
        .find_map(|line| line.strip_prefix("selected_cache_root="))
        .map(str::to_owned);
    Ok(ImportRun {
        status: result.status,
        stderr: String::from_utf8_lossy(&result.stderr).into_owned(),
        selected_root,
        initial_root,
        import_called: marker.exists(),
        _scratch: scratch,
    })
}

fn observer_steps() -> Result<Vec<velnor_actions_contract::Step>, String> {
    let rustc_identity = "a".repeat(64);
    let binding = QualificationObserverBinding {
        child_run_id: "901",
        child_attempt: "2",
        source_sha: "abcdef0123456789abcdef0123456789abcdef01",
        receipt_primary: PRIMARY,
        receipt_generation: GENERATION,
        receipt_rustc_identity: &rustc_identity,
        receipt_version: VERSION,
    };
    qualification_observer_steps(
        SCOPE,
        VERSION,
        &BTreeMap::from([("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned())]),
        &binding,
    )
    .map_err(|error| error.to_string())
}

fn executable(path: &std::path::Path, body: &str) -> Result<(), String> {
    fs::write(path, body).map_err(|error| io_error(&error))?;
    let mut permissions = fs::metadata(path)
        .map_err(|error| io_error(&error))?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|error| io_error(&error))
}

fn io_error(error: &std::io::Error) -> String {
    error.to_string()
}

struct ImportRun {
    status: std::process::ExitStatus,
    stderr: String,
    selected_root: Option<String>,
    initial_root: String,
    import_called: bool,
    _scratch: Scratch,
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(label: &str) -> Result<Self, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-mbx-observer-import-{}-{label}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).map_err(|error| io_error(&error))?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("could not clean MBX observer import test directory: {error}");
        }
    }
}
