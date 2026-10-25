use std::{
    env, fs,
    io::Write,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::ExitCode,
};
use velnor_actions_orchestrator::select_release_tap_continuation_from_env;

pub(super) fn gate_request(path: &Path) -> Option<PathBuf> {
    let temp = PathBuf::from(env::var_os("RUNNER_TEMP")?);
    if !temp.is_absolute()
        || path != temp.join("velnor/package-release-policy.json")
        || env::var_os("VELNOR_REQUEST_FILE").as_deref() != Some(path.as_os_str())
        || !matches!(
            env::var("GITHUB_EVENT_NAME").as_deref(),
            Ok("schedule" | "workflow_dispatch")
        )
    {
        return None;
    }
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 1024 * 1024 {
        return None;
    }
    for key in [
        "GH_TOKEN",
        "GITHUB_RUN_ID",
        "GITHUB_RUN_ATTEMPT",
        "GITHUB_WORKSPACE",
        "GITHUB_OUTPUT",
        "VELNOR_RELEASE_COMMIT_COUNT",
    ] {
        if env::var_os(key).is_none_or(|value| value.is_empty()) {
            return None;
        }
    }
    Some(path.to_path_buf())
}

pub(super) fn run() -> ExitCode {
    let selection = match select_release_tap_continuation_from_env() {
        Ok(value) => value,
        Err(error) => return super::fail_internal(&error.to_string()),
    };
    let output = selection.map_or_else(|| "eligible=false\n".to_owned(), |selected| format!(
        "eligible=true\nsource_sha={}\noriginal_run_id={}\noriginal_run_attempt={}\nci_run_id={}\nci_attempt={}\ncommit_count={}\n",
        selected.source_sha, selected.original_run_id, selected.original_run_attempt,
        selected.ci_run_id, selected.ci_attempt, selected.commit_count,
    ));
    match append_outputs(output.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => super::fail_internal(&format!("tap_continuation_output:{:?}", error.kind())),
    }
}

fn append_outputs(bytes: &[u8]) -> std::io::Result<()> {
    let path = PathBuf::from(env::var_os("GITHUB_OUTPUT").ok_or(std::io::ErrorKind::NotFound)?);
    if !path.is_absolute() {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    let original = fs::symlink_metadata(&path)?;
    if !original.is_file() || original.nlink() != 1 {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    let held = file.metadata()?;
    if !held.is_file()
        || held.dev() != original.dev()
        || held.ino() != original.ino()
        || held.nlink() != 1
    {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    file.write_all(bytes)
}
