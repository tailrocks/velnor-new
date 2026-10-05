use std::fs;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[derive(Debug, PartialEq, Eq)]
pub(in super::super) struct Outcome {
    pub(in super::super) status_code: Option<i32>,
    pub(in super::super) signal: Option<i32>,
    pub(in super::super) stdout: Vec<u8>,
    pub(in super::super) stderr: Vec<u8>,
    pub(in super::super) github_env: String,
    pub(in super::super) github_path: String,
    pub(in super::super) cwd_unchanged: bool,
    pub(in super::super) runner_entries: Vec<(String, Vec<u8>)>,
    pub(in super::super) child_survives: bool,
}

pub(in super::super) struct CasePaths {
    pub(in super::super) root: PathBuf,
    pub(in super::super) runner: PathBuf,
    pub(in super::super) tools: PathBuf,
    pub(in super::super) rust_root: PathBuf,
    pub(in super::super) github_env: PathBuf,
    pub(in super::super) github_path: PathBuf,
    pub(in super::super) cwd_capture: PathBuf,
    pub(in super::super) ready: PathBuf,
    pub(in super::super) child_pid: PathBuf,
    pub(in super::super) signal_capture: PathBuf,
}

pub(in super::super) fn case_paths(root: &Path) -> CasePaths {
    CasePaths {
        root: root.to_path_buf(),
        runner: root.join("runner-temp"),
        tools: root.join("tools"),
        rust_root: root.join("tools/rust-install"),
        github_env: root.join("github-env"),
        github_path: root.join("github-path"),
        cwd_capture: root.join("cwd-capture"),
        ready: root.join("mise-ready"),
        child_pid: root.join("mise-child-pid"),
        signal_capture: root.join("received-signal"),
    }
}

pub(in super::super) fn collect_outcome(
    paths: &CasePaths,
    output: Output,
    expect_child: bool,
) -> Outcome {
    let rust_root = paths.rust_root.to_string_lossy();
    let runner = paths.runner.to_string_lossy();
    let path_text = fs::read_to_string(&paths.github_path).expect("read GITHUB_PATH");
    let env_text = fs::read_to_string(&paths.github_env).expect("read GITHUB_ENV");
    let child_survives = if expect_child {
        let pid = fs::read_to_string(&paths.child_pid).expect("read child pid");
        pid_is_alive(pid.trim()).expect("probe child liveness")
    } else {
        false
    };
    Outcome {
        status_code: output.status.code(),
        signal: output.status.signal(),
        stdout: output.stdout,
        stderr: output.stderr,
        github_env: env_text.replace(runner.as_ref(), "<RUNNER_TEMP>"),
        github_path: path_text.replace(rust_root.as_ref(), "<RUST_ROOT>"),
        cwd_unchanged: fs::read_to_string(&paths.cwd_capture)
            .is_ok_and(|cwd| cwd.trim_end() == paths.root.to_string_lossy()),
        runner_entries: tree_entries(&paths.runner),
        child_survives,
    }
}

fn tree_entries(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut entries = Vec::new();
    walk_entries(root, root, &mut entries).expect("snapshot runner tree");
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

fn walk_entries(
    root: &Path,
    current: &Path,
    entries: &mut Vec<(String, Vec<u8>)>,
) -> io::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?
            .to_string_lossy()
            .into_owned();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.is_dir() {
            entries.push((format!("dir:{relative}"), Vec::new()));
            walk_entries(root, &path, entries)?;
        } else if metadata.is_file() {
            entries.push((format!("file:{relative}"), fs::read(&path)?));
        } else if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path)?;
            entries.push((
                format!("link:{relative}"),
                target.to_string_lossy().as_bytes().to_vec(),
            ));
        }
    }
    Ok(())
}

fn pid_is_alive(pid: &str) -> io::Result<bool> {
    let pid = pid
        .parse::<u32>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    if pid == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "child process id must be positive",
        ));
    }
    let output = Command::new("ps").args(["-axo", "pid=,stat="]).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "could not inspect child process state: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let state = String::from_utf8(output.stdout)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    for line in state.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace();
        let process = fields
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process id"))?
            .parse::<u32>()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        fields
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing process state"))?;
        if process == pid {
            return Ok(true);
        }
    }
    Ok(false)
}
