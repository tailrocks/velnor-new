// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use velnor_actions_mise::GitRequest;

const MAX_ARTIFACT_BYTES: usize = 64 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const MAX_FILES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationBinding {
    pub repository: String,
    pub workflow_sha: String,
    pub source_sha: String,
    pub run_id: u64,
    pub run_attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedFile {
    pub path: String,
    pub sha256: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparationArtifact {
    pub schema: u32,
    pub binding: PreparationBinding,
    pub files: Vec<PreparedFile>,
}

#[derive(Debug)]
pub struct PreparationCheckout {
    root: PathBuf,
    binding: PreparationBinding,
    allowed_paths: BTreeSet<String>,
    tracked: BTreeMap<String, TrackedFile>,
    git_files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrackedFile {
    mode: String,
    oid: String,
}

impl PreparationCheckout {
    pub fn open(
        root: &Path,
        binding: PreparationBinding,
        allowed_paths: BTreeSet<String>,
    ) -> Result<Self, &'static str> {
        if !valid_binding(&binding) || allowed_paths.iter().any(|path| !valid_path(path)) {
            return Err("release_pr_checkout_invalid");
        }
        crate::verify_crate_release_source(root, &binding.source_sha)
            .map_err(|_| "release_pr_checkout_invalid")?;
        let root = root
            .canonicalize()
            .map_err(|_| "release_pr_checkout_invalid")?;
        let tracked = tracked_files(&root)?;
        let git_files = git_file_digests(&root)?;
        Ok(Self {
            root,
            binding,
            allowed_paths,
            tracked,
            git_files,
        })
    }

    pub fn collect(self) -> Result<PreparationArtifact, &'static str> {
        self.collect_with(|| {})
    }

    fn collect_with(self, between: impl FnOnce()) -> Result<PreparationArtifact, &'static str> {
        self.ensure_current()?;
        let changed = self.capture_changes()?;
        between();
        self.ensure_current()?;
        if self.capture_changes()? != changed {
            return Err("release_pr_checkout_changed");
        }
        self.ensure_current()?;
        let artifact = PreparationArtifact {
            schema: 1,
            binding: self.binding,
            files: changed.into_values().collect(),
        };
        let bytes = serde_json::to_vec(&artifact).map_err(|_| "release_pr_artifact_invalid")?;
        verify_preparation_artifact(
            &bytes,
            &sha256(&bytes),
            &artifact.binding,
            &self.allowed_paths,
        )
    }

    fn ensure_current(&self) -> Result<(), &'static str> {
        let head = git_output(&self.root, &GitRequest::rev_parse(vec!["HEAD".into()]))?;
        if head != format!("{}\n", self.binding.source_sha).as_bytes()
            || tracked_files(&self.root)? != self.tracked
            || git_file_digests(&self.root)? != self.git_files
        {
            return Err("release_pr_checkout_changed");
        }
        Ok(())
    }

    fn capture_changes(&self) -> Result<BTreeMap<String, PreparedFile>, &'static str> {
        let mut changed = BTreeMap::new();
        let mut changed_bytes = 0usize;
        for (path, tracked) in &self.tracked {
            let bytes = read_tracked(&self.root, path, &tracked.mode)?;
            let oid = crate::release_policy_admission::git_blob_sha1(&bytes)
                .map_err(|_| "release_pr_checkout_invalid")?;
            if oid != tracked.oid {
                if !self.allowed_paths.contains(path) || tracked.mode == "120000" {
                    return Err("release_pr_checkout_unapproved_change");
                }
                insert_change(&mut changed, &mut changed_bytes, path, bytes)?;
            }
        }
        for path in untracked_files(&self.root)? {
            if !self.allowed_paths.contains(&path) {
                return Err("release_pr_checkout_unapproved_change");
            }
            let bytes = read_tracked(&self.root, &path, "100644")?;
            insert_change(&mut changed, &mut changed_bytes, &path, bytes)?;
        }
        Ok(changed)
    }
}

fn insert_change(
    changed: &mut BTreeMap<String, PreparedFile>,
    total: &mut usize,
    path: &str,
    bytes: Vec<u8>,
) -> Result<(), &'static str> {
    *total = total
        .checked_add(bytes.len())
        .ok_or("release_pr_artifact_invalid")?;
    if bytes.len() > MAX_FILE_BYTES || *total > MAX_TOTAL_BYTES || changed.len() >= MAX_FILES {
        return Err("release_pr_artifact_invalid");
    }
    if changed
        .insert(
            path.to_owned(),
            PreparedFile {
                path: path.to_owned(),
                sha256: sha256(&bytes),
                bytes,
            },
        )
        .is_some()
    {
        return Err("release_pr_artifact_invalid");
    }
    Ok(())
}

fn git_output(root: &Path, request: &GitRequest) -> Result<Vec<u8>, &'static str> {
    let result = request
        .run_release_in(root)
        .map_err(|_| "release_pr_checkout_git")?;
    if !result.success {
        return Err("release_pr_checkout_git");
    }
    Ok(result.stdout)
}

fn tracked_files(root: &Path) -> Result<BTreeMap<String, TrackedFile>, &'static str> {
    let output = git_output(
        root,
        &GitRequest::ls_tree(vec![
            "-r".into(),
            "-z".into(),
            "--full-tree".into(),
            "HEAD".into(),
        ]),
    )?;
    let mut files = BTreeMap::new();
    for entry in output
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
    {
        let record = std::str::from_utf8(entry).map_err(|_| "release_pr_checkout_invalid")?;
        let (header, path) = record
            .split_once('\t')
            .ok_or("release_pr_checkout_invalid")?;
        let fields: Vec<_> = header.split(' ').collect();
        if fields.len() != 3
            || fields[1] != "blob"
            || !valid_path(path)
            || !matches!(fields[0], "100644" | "100755" | "120000")
            || !valid_git_sha(fields[2])
            || files
                .insert(
                    path.to_owned(),
                    TrackedFile {
                        mode: fields[0].into(),
                        oid: fields[2].into(),
                    },
                )
                .is_some()
        {
            return Err("release_pr_checkout_invalid");
        }
    }
    Ok(files)
}

fn untracked_files(root: &Path) -> Result<Vec<String>, &'static str> {
    let output = git_output(
        root,
        &GitRequest::ls_files(vec!["--others".into(), "-z".into()]),
    )?;
    output
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|bytes| {
            let path = std::str::from_utf8(bytes).map_err(|_| "release_pr_checkout_invalid")?;
            if !valid_path(path) {
                return Err("release_pr_checkout_invalid");
            }
            Ok(path.to_owned())
        })
        .collect()
}

fn git_file_digests(root: &Path) -> Result<BTreeMap<String, String>, &'static str> {
    let git = root.join(".git");
    let metadata = fs::symlink_metadata(&git).map_err(|_| "release_pr_checkout_git")?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("release_pr_checkout_git");
    }
    let mut hashes = BTreeMap::new();
    for path in ["HEAD", "config"] {
        let bytes = read_regular(&git, path, 64 * 1024 * 1024)?;
        hashes.insert(path.to_owned(), sha256(&bytes));
    }
    let index = git_output(
        root,
        &GitRequest::ls_files(vec!["--stage".into(), "-z".into()]),
    )?;
    hashes.insert("index".to_owned(), sha256(&index));
    Ok(hashes)
}

fn read_tracked(root: &Path, path: &str, mode: &str) -> Result<Vec<u8>, &'static str> {
    check_parent_dirs(root, path)?;
    let metadata =
        fs::symlink_metadata(root.join(path)).map_err(|_| "release_pr_checkout_changed")?;
    if mode == "120000" {
        if !metadata.file_type().is_symlink() {
            return Err("release_pr_checkout_changed");
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt as _;
            return Ok(fs::read_link(root.join(path))
                .map_err(|_| "release_pr_checkout_changed")?
                .as_os_str()
                .as_bytes()
                .to_vec());
        }
        #[cfg(not(unix))]
        return Err("release_pr_checkout_changed");
    }
    if !metadata.is_file() || metadata.file_type().is_symlink() || !mode_matches(&metadata, mode) {
        return Err("release_pr_checkout_changed");
    }
    read_regular(root, path, 256 * 1024 * 1024)
}

fn check_parent_dirs(root: &Path, path: &str) -> Result<(), &'static str> {
    let mut parent = root.to_path_buf();
    let Some((dirs, _)) = path.rsplit_once('/') else {
        return Ok(());
    };
    for part in dirs.split('/') {
        parent.push(part);
        let metadata = fs::symlink_metadata(&parent).map_err(|_| "release_pr_checkout_changed")?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("release_pr_checkout_changed");
        }
    }
    Ok(())
}

#[cfg(unix)]
fn mode_matches(metadata: &fs::Metadata, mode: &str) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    let executable = metadata.permissions().mode() & 0o111 != 0;
    (mode == "100755") == executable
}

#[cfg(not(unix))]
fn mode_matches(_metadata: &fs::Metadata, _mode: &str) -> bool {
    false
}

fn read_regular(root: &Path, path: &str, limit: u64) -> Result<Vec<u8>, &'static str> {
    match crate::safe_read::read_repo_bytes(root, path, limit)
        .map_err(|_| "release_pr_checkout_changed")?
    {
        crate::safe_read::RepoBytes::Bytes(bytes) => Ok(bytes),
        crate::safe_read::RepoBytes::Absent => Err("release_pr_checkout_changed"),
    }
}

pub fn verify_preparation_artifact(
    bytes: &[u8],
    digest: &str,
    binding: &PreparationBinding,
    allowed_paths: &BTreeSet<String>,
) -> Result<PreparationArtifact, &'static str> {
    let invalid = || "release_pr_artifact_invalid";
    if bytes.len() > MAX_ARTIFACT_BYTES || digest != sha256(bytes) || !valid_binding(binding) {
        return Err(invalid());
    }
    let artifact: PreparationArtifact = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if artifact.schema != 1 || artifact.binding != *binding || artifact.files.len() > MAX_FILES {
        return Err(invalid());
    }
    let mut previous: Option<&str> = None;
    let mut total = 0usize;
    for file in &artifact.files {
        if !valid_path(&file.path)
            || !allowed_paths.contains(&file.path)
            || previous.is_some_and(|prior| prior >= file.path.as_str())
            || file.bytes.len() > MAX_FILE_BYTES
            || file.sha256 != sha256(&file.bytes)
        {
            return Err(invalid());
        }
        total = total.checked_add(file.bytes.len()).ok_or_else(invalid)?;
        if total > MAX_TOTAL_BYTES {
            return Err(invalid());
        }
        previous = Some(&file.path);
    }
    Ok(artifact)
}

fn valid_binding(binding: &PreparationBinding) -> bool {
    let Some((owner, repository)) = binding.repository.split_once('/') else {
        return false;
    };
    velnor_actions_contract::config::is_valid_github_owner(owner)
        && velnor_actions_contract::config::is_valid_github_repo_name(repository)
        && valid_git_sha(&binding.workflow_sha)
        && valid_git_sha(&binding.source_sha)
        && binding.run_id > 0
        && binding.run_attempt > 0
}

fn valid_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains('\\')
        && !path.bytes().any(|byte| byte.is_ascii_control())
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && part != ".git")
}

fn sha256(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 15)]));
    }
    result
}

fn execute_preparation_command(
    checkout: PreparationCheckout,
    command: &velnor_actions_mise::command::IsolatedCommand,
    timeout: std::time::Duration,
) -> Result<PreparationArtifact, &'static str> {
    let deadline = velnor_actions_mise::CheckDeadline::after(timeout)
        .map_err(|_| "release_pr_update_failed")?;
    let (output, _drained) = command
        .run_quiescent_until(1024 * 1024, deadline)
        .map_err(|_| "release_pr_update_failed")?;
    if !output.success {
        return Err("release_pr_update_failed");
    }
    checkout.collect()
}

pub fn prepare_pr_from_environment() -> Result<(), crate::OrchestratorError> {
    prepare_pr_inner().map_err(crate::internal::internal)
}

fn prepare_pr_inner() -> Result<(), &'static str> {
    use velnor_actions_workflow_renderer::release_config::{
        ReleasePlzConfig, ReleasePlzPackage, render_release_plz_config,
    };
    let invalid = || "release_pr_preparation_invalid";
    let required = |key: &str| {
        std::env::var(key)
            .ok()
            .filter(|value| !value.is_empty())
            .ok_or_else(invalid)
    };
    let source = PathBuf::from(required("GITHUB_WORKSPACE")?)
        .canonicalize()
        .map_err(|_| invalid())?;
    let temp = PathBuf::from(required("RUNNER_TEMP")?)
        .canonicalize()
        .map_err(|_| invalid())?;
    if temp.starts_with(&source) || !temp.is_dir() {
        return Err(invalid());
    }
    let intent_bytes = read_regular(&source, ".github/velnor-release-intent.toml", 1024 * 1024)?;
    let intent = crate::parse_crate_release_intent(&intent_bytes).map_err(|_| invalid())?;
    let source_sha = required("GITHUB_SHA")?;
    let run_id = required("GITHUB_RUN_ID")?
        .parse::<u64>()
        .map_err(|_| invalid())?;
    let run_attempt = required("GITHUB_RUN_ATTEMPT")?
        .parse::<u32>()
        .map_err(|_| invalid())?;
    if required("GITHUB_EVENT_NAME")? != "push"
        || required("GITHUB_REPOSITORY")? != intent.repository
        || required("GITHUB_REF")? != format!("refs/heads/{}", intent.default_branch)
        || run_attempt != 1
    {
        return Err(invalid());
    }
    let binding = PreparationBinding {
        repository: intent.repository.clone(),
        workflow_sha: source_sha.clone(),
        source_sha,
        run_id,
        run_attempt,
    };
    let allowed: BTreeSet<String> = intent.pr_write_paths.iter().cloned().collect();
    let checkout = PreparationCheckout::open(&source, binding, allowed)?;
    let config = ReleasePlzConfig {
        tag_pattern: intent.tag_template.clone(),
        semver_check: true,
        packages: intent
            .packages
            .iter()
            .map(|name| ReleasePlzPackage {
                name: name.clone(),
                publish_features: intent.publish_features[name].clone(),
            })
            .collect(),
    };
    let expected =
        render_release_plz_config(&config, &intent.generator_version).map_err(|_| invalid())?;
    if read_regular(&source, ".github/release-plz.toml", 1024 * 1024)? != expected.as_bytes() {
        return Err(invalid());
    }
    let artifact = run_preparation_update(checkout, &intent, &source, &temp)?;
    let output = PathBuf::from(required("GITHUB_OUTPUT")?);
    write_preparation_artifact(&artifact, &temp, &output)
}

fn write_preparation_artifact(
    artifact: &PreparationArtifact,
    temp: &Path,
    output: &Path,
) -> Result<(), &'static str> {
    use std::io::Write as _;
    let invalid = || "release_pr_preparation_invalid";
    let bytes = serde_json::to_vec(&artifact).map_err(|_| invalid())?;
    if bytes.len() > MAX_ARTIFACT_BYTES {
        return Err(invalid());
    }
    let artifact_path = temp.join("velnor-release-pr-preparation.json");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(artifact_path)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|_| invalid())?;
    if !output.is_absolute() {
        return Err(invalid());
    }
    let mut output = fs::OpenOptions::new()
        .append(true)
        .open(output)
        .map_err(|_| invalid())?;
    writeln!(output, "content_sha256={}", sha256(&bytes)).map_err(|_| invalid())?;
    writeln!(output, "changed={}", !artifact.files.is_empty()).map_err(|_| invalid())?;
    Ok(())
}

fn run_preparation_update(
    checkout: PreparationCheckout,
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
    source: &Path,
    temp: &Path,
) -> Result<PreparationArtifact, &'static str> {
    let invalid = || "release_pr_preparation_invalid";
    let plz = temp.join("velnor-release-plz/release-plz");
    let semver = temp.join("velnor-semver/cargo-semver-checks");
    let plz_pin = velnor_actions_mise::catalog::qualified_release_plz(
        &intent.release_plz_version,
        &intent.runner,
    )
    .ok_or_else(invalid)?;
    let semver_pin = velnor_actions_mise::catalog::qualified_cargo_semver_checks(&intent.runner)
        .ok_or_else(invalid)?;
    check_pinned_executable(&plz, plz_pin.executable_sha256)?;
    check_pinned_executable(&semver, semver_pin.executable_sha256)?;
    let rustup = temp.join("velnor/rustup");
    let cargo = crate::crate_release_source::verified_rust_tool_path(
        &rustup,
        &intent.rust_version,
        "cargo",
    )
    .map_err(|_| invalid())?;
    let rustc = crate::crate_release_source::verified_rust_tool_path(
        &rustup,
        &intent.rust_version,
        "rustc",
    )
    .map_err(|_| invalid())?;
    let rustdoc = crate::crate_release_source::verified_rust_tool_path(
        &rustup,
        &intent.rust_version,
        "rustdoc",
    )
    .map_err(|_| invalid())?;
    let rust_hashes: Vec<_> = [&cargo, &rustc, &rustdoc]
        .iter()
        .map(|path| {
            fs::read(path)
                .map(|bytes| sha256(&bytes))
                .map_err(|_| invalid())
        })
        .collect::<Result<_, _>>()?;
    let scratch = tempfile::Builder::new()
        .prefix("velnor-pr-")
        .tempdir_in(temp)
        .map_err(|_| invalid())?;
    let tools = PreparationToolPaths {
        cargo: &cargo,
        rustc: &rustc,
        rustdoc: &rustdoc,
        rustup: &rustup,
        semver: &semver,
        temp,
    };
    let env = preparation_tool_environment(&tools, scratch.path())?;
    let args = vec![
        "update".into(),
        "--config".into(),
        source.join(".github/release-plz.toml").into_os_string(),
        "--manifest-path".into(),
        source.join(&intent.manifest_path).into_os_string(),
    ];
    let command =
        velnor_actions_mise::command::IsolatedCommand::crate_update(&plz, args, source, &env)
            .map_err(|_| invalid())?;
    let artifact =
        execute_preparation_command(checkout, &command, std::time::Duration::from_mins(20))?;
    check_pinned_executable(&plz, plz_pin.executable_sha256)?;
    check_pinned_executable(&semver, semver_pin.executable_sha256)?;
    for (path, expected) in [&cargo, &rustc, &rustdoc].iter().zip(rust_hashes) {
        if sha256(&fs::read(path).map_err(|_| invalid())?) != expected {
            return Err(invalid());
        }
    }
    Ok(artifact)
}

struct PreparationToolPaths<'a> {
    cargo: &'a Path,
    rustc: &'a Path,
    rustdoc: &'a Path,
    rustup: &'a Path,
    semver: &'a Path,
    temp: &'a Path,
}

fn preparation_tool_environment(
    tools: &PreparationToolPaths<'_>,
    scratch: &Path,
) -> Result<Vec<(std::ffi::OsString, std::ffi::OsString)>, &'static str> {
    use std::ffi::OsString;
    let invalid = || "release_pr_preparation_invalid";
    let home = scratch.join("home");
    let cargo_home = scratch.join("cargo");
    let target = scratch.join("target");
    for dir in [&home, &cargo_home, &target] {
        fs::create_dir(dir).map_err(|_| invalid())?;
    }
    let path = std::env::join_paths([
        tools.semver.parent().ok_or_else(invalid)?,
        tools.cargo.parent().ok_or_else(invalid)?,
        Path::new("/usr/bin"),
        Path::new("/bin"),
    ])
    .map_err(|_| invalid())?;
    let env: Vec<(OsString, OsString)> = vec![
        ("PATH".into(), path),
        ("CARGO".into(), tools.cargo.as_os_str().into()),
        ("RUSTC".into(), tools.rustc.as_os_str().into()),
        ("RUSTDOC".into(), tools.rustdoc.as_os_str().into()),
        ("HOME".into(), home.as_os_str().into()),
        ("CARGO_HOME".into(), cargo_home.as_os_str().into()),
        ("CARGO_TARGET_DIR".into(), target.as_os_str().into()),
        ("RUSTUP_HOME".into(), tools.rustup.as_os_str().into()),
        ("RUSTUP_AUTO_INSTALL".into(), "0".into()),
        ("RUNNER_TEMP".into(), tools.temp.as_os_str().into()),
    ];
    Ok(env)
}

fn check_pinned_executable(path: &Path, expected: &str) -> Result<(), &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "release_pr_tool_invalid")?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > 256 * 1024 * 1024
    {
        return Err("release_pr_tool_invalid");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err("release_pr_tool_invalid");
        }
    }
    if sha256(&fs::read(path).map_err(|_| "release_pr_tool_invalid")?) != expected {
        return Err("release_pr_tool_invalid");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{PreparationBinding, PreparationCheckout};
    use std::{collections::BTreeSet, fs, path::Path, process::Command};

    fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
        let result = Command::new("git")
            .current_dir(root)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(args)
            .output()?;
        if !result.status.success() {
            return Err("git failed".into());
        }
        Ok(String::from_utf8(result.stdout)?.trim().to_owned())
    }

    fn make_checkout()
    -> Result<(tempfile::TempDir, PreparationCheckout), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        git(root.path(), &["init", "-q"])?;
        fs::write(root.path().join("Cargo.toml"), "base\n")?;
        fs::write(root.path().join("release-plz.toml"), "")?;
        git(root.path(), &["add", "."])?;
        git(
            root.path(),
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "initial",
            ],
        )?;
        let sha = git(root.path(), &["rev-parse", "HEAD"])?;
        let binding = PreparationBinding {
            repository: "owner/project".into(),
            workflow_sha: sha.clone(),
            source_sha: sha,
            run_id: 12,
            run_attempt: 1,
        };
        let checkout =
            PreparationCheckout::open(root.path(), binding, BTreeSet::from(["Cargo.toml".into()]))?;
        Ok((root, checkout))
    }

    #[test]
    fn git_index_refresh_is_allowed_but_staged_changes_are_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let (root, checkout) = make_checkout()?;
        fs::write(root.path().join("Cargo.toml"), "base\n")?;
        git(root.path(), &["update-index", "--refresh"])?;
        assert_eq!(checkout.collect()?.files, []);
        let (root, checkout) = make_checkout()?;
        fs::write(root.path().join("Cargo.toml"), "changed\n")?;
        git(root.path(), &["add", "Cargo.toml"])?;
        assert!(checkout.collect().is_err());
        Ok(())
    }

    #[test]
    fn rejects_source_and_git_changes_between_snapshots() -> Result<(), Box<dyn std::error::Error>>
    {
        let (root, checkout) = make_checkout()?;
        fs::write(root.path().join("Cargo.toml"), "update\n")?;
        let path = root.path().join("Cargo.toml");
        assert!(
            checkout
                .collect_with(|| fs::write(&path, "changed again\n").expect("fixture write"))
                .is_err()
        );

        let (root, checkout) = make_checkout()?;
        let config = root.path().join(".git/config");
        let mut bytes = fs::read(&config)?;
        bytes.push(b'\n');
        assert!(
            checkout
                .collect_with(|| fs::write(&config, bytes).expect("fixture write"))
                .is_err()
        );
        Ok(())
    }
    #[cfg(unix)]
    fn update_command(
        source: &Path,
        tools: &Path,
        script: &str,
    ) -> Result<velnor_actions_mise::command::IsolatedCommand, Box<dyn std::error::Error>> {
        use std::{ffi::OsString, os::unix::fs::PermissionsExt as _};
        let program = tools.join("release-plz");
        fs::write(&program, script)?;
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755))?;
        let tool_home = tools.to_str().ok_or("tools")?;
        let env: Vec<(OsString, OsString)> = [
            ("PATH", "/usr/bin:/bin"),
            ("CARGO", "/usr/bin/true"),
            ("RUSTC", "/usr/bin/true"),
            ("RUSTDOC", "/usr/bin/true"),
            ("HOME", tool_home),
            ("CARGO_HOME", tool_home),
            ("CARGO_TARGET_DIR", tool_home),
            ("RUSTUP_HOME", tool_home),
            ("RUSTUP_AUTO_INSTALL", "0"),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect();
        let args = vec![
            "update".into(),
            "--config".into(),
            source.join("release-plz.toml").into_os_string(),
            "--manifest-path".into(),
            source.join("Cargo.toml").into_os_string(),
        ];
        Ok(velnor_actions_mise::command::IsolatedCommand::crate_update(
            &program, args, source, &env,
        )?)
    }

    #[cfg(unix)]
    #[test]
    fn updater_success_is_captured_but_failure_never_produces_artifact()
    -> Result<(), Box<dyn std::error::Error>> {
        let (root, checkout) = make_checkout()?;
        let tools = tempfile::tempdir()?;
        let cmd = update_command(
            root.path(),
            tools.path(),
            "#!/bin/sh\nprintf 'update\\n' > Cargo.toml\n",
        )?;
        let artifact =
            super::execute_preparation_command(checkout, &cmd, std::time::Duration::from_secs(10))?;
        assert_eq!(artifact.files.len(), 1);
        assert_eq!(artifact.files[0].bytes, b"update\n");
        let (root, checkout) = make_checkout()?;
        let tools = tempfile::tempdir()?;
        let cmd = update_command(
            root.path(),
            tools.path(),
            "#!/bin/sh\nprintf 'partial\\n' > Cargo.toml\nexit 1\n",
        )?;
        assert!(
            super::execute_preparation_command(checkout, &cmd, std::time::Duration::from_secs(10))
                .is_err()
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn updater_children_are_drained_and_timeout_is_not_captured()
    -> Result<(), Box<dyn std::error::Error>> {
        let (root, checkout) = make_checkout()?;
        let tools = tempfile::tempdir()?;
        let cmd = update_command(
            root.path(),
            tools.path(),
            "#!/bin/sh\nprintf 'ready\\n' > Cargo.toml\n(sleep 2; printf 'late\\n' > Cargo.toml) >/dev/null 2>&1 &\n",
        )?;
        let artifact =
            super::execute_preparation_command(checkout, &cmd, std::time::Duration::from_secs(10))?;
        assert_eq!(artifact.files[0].bytes, b"ready\n");
        std::thread::sleep(std::time::Duration::from_secs(3));
        assert_eq!(fs::read(root.path().join("Cargo.toml"))?, b"ready\n");

        let (root, checkout) = make_checkout()?;
        let tools = tempfile::tempdir()?;
        let cmd = update_command(root.path(), tools.path(), "#!/bin/sh\nsleep 5\n")?;
        assert!(
            super::execute_preparation_command(checkout, &cmd, std::time::Duration::from_secs(1))
                .is_err()
        );
        Ok(())
    }
}
