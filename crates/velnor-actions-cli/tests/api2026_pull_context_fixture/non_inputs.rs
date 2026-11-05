// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    io::Read as _,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[path = "capture_verify.rs"]
pub(crate) mod capture_verify;

pub(super) struct PreparedSource {
    _temporary: tempfile::TempDir,
    pub(super) driver: PathBuf,
    pub(super) live_root: PathBuf,
    pub(super) validation: PathBuf,
    pub(super) manifest: PathBuf,
    pub(super) patterns: String,
}

impl PreparedSource {
    pub(super) fn command(&self) -> Command {
        let mut command = Command::new("/usr/bin/python3");
        command
            .args(["-I", "-S", "-B"])
            .arg(&self.driver)
            .env("VELNOR_FIXTURE_NON_INPUTS", &self.patterns)
            .env("VELNOR_FIXTURE_LIVE_ROOT", &self.live_root)
            .env("VELNOR_FIXTURE_VALIDATION_ROOT", &self.validation)
            .env("VELNOR_FIXTURE_CAPTURE_MANIFEST", &self.manifest);
        command
    }
}

pub(super) fn prepare_source() -> Result<PreparedSource, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root missing")?;
    let temporary = tempfile::tempdir()?;
    let code = temporary.path().join("code");
    let validation = temporary.path().join("validation");
    let manifest = temporary.path().join("manifest.json");
    std::fs::create_dir(&code)?;
    std::fs::create_dir(&validation)?;
    for (name, bytes) in bundled_files() {
        std::fs::write(code.join(name), bytes)?;
    }
    let mut command = Command::new("/usr/bin/python3");
    command
        .args(["-I", "-S", "-B"])
        .arg(code.join("raw_capture.py"))
        .arg(root)
        .arg(&validation)
        .arg(&manifest)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/nonexistent");
    let deadline = velnor_actions_mise::CheckDeadline::after(Duration::from_secs(180))?;
    let (result, _) = velnor_actions_mise::command::run_prepared_quiescent_until(
        &mut command,
        1024 * 1024,
        deadline,
    )?;
    if !result.success {
        return Err("fixture source capture failed".into());
    }
    let text = read_manifest(&manifest)?;
    capture_verify::verify_capture(&validation, &text)?;
    verify_bundled_files(&validation)?;
    let patterns = reviewed_non_inputs_from(&validation)?;
    capture_verify::verify_capture(&validation, &text)?;
    verify_bundled_files(&validation)?;
    if read_manifest(&manifest)? != text {
        return Err("fixture capture manifest changed".into());
    }
    Ok(PreparedSource {
        _temporary: temporary,
        driver: code.join("driver.py"),
        live_root: root.to_path_buf(),
        validation,
        manifest,
        patterns,
    })
}

fn bundled_files() -> [(&'static str, &'static [u8]); 5] {
    [
        ("raw_capture.py", &include_bytes!("raw_capture.py")[..]),
        ("driver.py", &include_bytes!("driver.py")[..]),
        (
            "source_snapshot.py",
            &include_bytes!("source_snapshot.py")[..],
        ),
        ("docker_host.py", &include_bytes!("docker_host.py")[..]),
        ("tools.json", &include_bytes!("tools.json")[..]),
    ]
}

fn verify_bundled_files(root: &Path) -> Result<(), Box<dyn Error>> {
    let directory = root.join("crates/velnor-actions-cli/tests/api2026_pull_context_fixture");
    for (name, expected) in bundled_files() {
        if std::fs::read(directory.join(name))? != expected {
            return Err("captured fixture code differs from compiled harness".into());
        }
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<String, Box<dyn Error>> {
    use rustix::fs::{FileType, Mode, OFlags, fstat, open};
    let file = open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    let stat = fstat(&file)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_nlink != 1
        || stat.st_mode & 0o077 != 0
        || stat.st_uid != rustix::process::geteuid().as_raw()
    {
        return Err("invalid fixture capture manifest".into());
    }
    let mut bytes = Vec::new();
    let mut file = std::fs::File::from(file);
    (&mut file)
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    let after = fstat(&file)?;
    if bytes.len() > 64 * 1024 * 1024
        || u64::try_from(bytes.len())? != u64::try_from(stat.st_size)?
        || stat.st_ino != after.st_ino
        || stat.st_dev != after.st_dev
        || stat.st_mode != after.st_mode
        || stat.st_size != after.st_size
        || stat.st_mtime != after.st_mtime
        || stat.st_mtime_nsec != after.st_mtime_nsec
    {
        return Err("fixture capture manifest too large".into());
    }
    Ok(String::from_utf8(bytes)?)
}

pub(super) fn reviewed_non_inputs_from(root: &Path) -> Result<String, Box<dyn Error>> {
    let config = velnor_actions_orchestrator::prepare(root)?.config;
    let patterns = config
        .stacks
        .rust
        .as_ref()
        .map_or(&[][..], |rust| rust.non_inputs.globs());
    Ok(serde_json::to_string(patterns)?)
}

#[cfg(test)]
mod tests {
    use super::{bundled_files, verify_bundled_files};

    #[test]
    fn captured_harness_must_match_compiled_bytes() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let directory = root
            .path()
            .join("crates/velnor-actions-cli/tests/api2026_pull_context_fixture");
        std::fs::create_dir_all(&directory)?;
        for (name, bytes) in bundled_files() {
            std::fs::write(directory.join(name), bytes)?;
        }
        verify_bundled_files(root.path())?;
        std::fs::write(directory.join("driver.py"), b"changed after compilation")?;
        assert!(verify_bundled_files(root.path()).is_err());
        Ok(())
    }
}
