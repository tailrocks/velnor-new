use super::{LINUX, qualification_script, verify_provenance_in_directory};
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

#[test]
fn wrong_checksum_filename_stops_before_candidate_execution() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let directory_name = format!("target/release-sidecar-test-{}-{nonce}", std::process::id());
    let directory = root.join(&directory_name);
    fs::create_dir_all(&directory)?;
    let scratch = Scratch(directory.clone());
    let executed = scratch.0.join("candidate-executed");
    let candidate = directory.join(LINUX.binary);
    fs::write(
        &candidate,
        "#!/bin/sh\nprintf '%s\\n' executed >> \"$CANDIDATE_EXECUTED\"\nprintf '%s\\n' 'velnor-actions 0.1.1'\n",
    )?;
    let mut permissions = fs::metadata(&candidate)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&candidate, permissions)?;
    fs::write(
        directory.join(LINUX.sidecar),
        format!("{}  unrelated-binary\n", "a".repeat(64)),
    )?;
    let sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root)
        .output()?;
    if !sha.status.success() {
        return Err("cannot read candidate source SHA".into());
    }
    let source_sha = String::from_utf8(sha.stdout)?.trim().to_owned();
    let command = format!(
        "{}\n{}",
        verify_provenance_in_directory(LINUX, &directory_name, "1.98.1", "1.21.1"),
        qualification_script(LINUX.binary, &directory_name)
    );
    let status = Command::new("bash")
        .arg("-c")
        .arg(command)
        .current_dir(&root)
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_SHA", source_sha)
        .env("GITHUB_WORKSPACE", &root)
        .env("CANDIDATE_EXECUTED", &executed)
        .status()?;
    assert!(!status.success(), "accepted a sidecar for another filename");
    assert!(
        !executed.exists(),
        "candidate ran before its sidecar passed validation"
    );
    Ok(())
}
