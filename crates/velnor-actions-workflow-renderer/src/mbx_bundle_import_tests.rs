use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{IMPORT_SCRIPT, import_step, step_yaml_id};

#[test]
fn import_step_exposes_selected_root_on_each_branch() -> Result<(), String> {
    let step = import_step(None).map_err(|error| error.to_string())?;
    assert_eq!(step_yaml_id(&step), Some("mbx-bundle-import"));

    for (label, matched, bundle, status, expected_fallback) in [
        ("miss", "", false, 0, false),
        ("missing", "true", false, 0, false),
        ("success", "true", true, 0, false),
        ("fallback", "true", true, 23, true),
    ] {
        let run = run_import(label, matched, bundle, status)?;
        assert!(
            run.status.success(),
            "{label}: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        if expected_fallback {
            assert_ne!(run.selected_root, run.initial_root);
            assert!(run.selected_root.starts_with(&run.runner_temp));
            assert!(std::path::Path::new(&run.selected_root).is_dir());
            assert!(
                run.github_env
                    .contains(&format!("MBX_CACHE_DIR={}\n", run.selected_root))
            );
        } else {
            assert_eq!(run.selected_root, run.initial_root, "{label}");
        }
    }
    Ok(())
}

fn run_import(
    label: &str,
    matched: &str,
    bundle_exists: bool,
    import_status: u8,
) -> Result<ImportRun, String> {
    let scratch = Scratch::new(label)?;
    let runner_temp = scratch.0.join("runner-temp");
    let bin = scratch.0.join("bin");
    fs::create_dir_all(&runner_temp).map_err(|error| error.to_string())?;
    fs::create_dir_all(&bin).map_err(|error| error.to_string())?;
    if bundle_exists {
        fs::create_dir_all(runner_temp.join("mbx-single-bundle"))
            .map_err(|error| error.to_string())?;
    }
    executable(
        &bin.join("mbx"),
        "#!/bin/sh\nprintf 'imported bundle\\n'\nexit \"${IMPORT_STATUS:-0}\"\n",
    )?;
    executable(&bin.join("df"), "#!/bin/sh\nexit 0\n")?;
    let output = scratch.0.join("github-output");
    let github_env = scratch.0.join("github-env");
    fs::write(&output, "").map_err(|error| error.to_string())?;
    fs::write(&github_env, "").map_err(|error| error.to_string())?;
    let initial_root = runner_temp.join("initial-cache-root");
    let result = Command::new("/bin/bash")
        .arg("-c")
        .arg(IMPORT_SCRIPT)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env("RUNNER_TEMP", &runner_temp)
        .env("GITHUB_OUTPUT", &output)
        .env("GITHUB_ENV", &github_env)
        .env("GITHUB_RUN_ID", "91")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("MBX_CACHE_DIR", &initial_root)
        .env("MBX_TARGET_ROOT", initial_root.join("targets"))
        .env("MBX_SHIMS_DIR", initial_root.join("shims"))
        .env("MBX_CACHE_EXPORT_GROUP", "initial-group")
        .env("MATCHED", matched)
        .env("IMPORT_STATUS", import_status.to_string())
        .output()
        .map_err(|error| error.to_string())?;
    let output_text = fs::read_to_string(output).map_err(|error| error.to_string())?;
    let selected_root = output_text
        .lines()
        .find_map(|line| line.strip_prefix("selected_cache_root="))
        .ok_or_else(|| format!("{label}: selected_cache_root missing from {output_text}"))?
        .to_owned();
    let github_env = fs::read_to_string(github_env).map_err(|error| error.to_string())?;
    Ok(ImportRun {
        status: result.status,
        stderr: result.stderr,
        initial_root: initial_root.display().to_string(),
        runner_temp: runner_temp.display().to_string(),
        selected_root,
        github_env,
        _scratch: scratch,
    })
}

struct ImportRun {
    status: std::process::ExitStatus,
    stderr: Vec<u8>,
    initial_root: String,
    runner_temp: String,
    selected_root: String,
    github_env: String,
    _scratch: Scratch,
}

fn executable(path: &std::path::Path, body: &str) -> Result<(), String> {
    fs::write(path, body).map_err(|error| error.to_string())?;
    let mut permissions = fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|error| error.to_string())
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(label: &str) -> Result<Self, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-mbx-import-{}-{label}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("could not clean MBX import test directory: {error}");
        }
    }
}
