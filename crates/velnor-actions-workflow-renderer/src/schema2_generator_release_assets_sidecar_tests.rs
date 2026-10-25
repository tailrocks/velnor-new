use super::super::{
    LINUX, VERSION, qualification_script, verify_authority_provenance_script,
    verify_provenance_in_directory,
};
use super::{Scratch, TOKEN_SENTINEL, run_step, write_executable};
use crate::schema2::git_fixture;

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn wrong_checksum_filename_stops_before_candidate_execution() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let scratch = Scratch::new()?;
    let directory_name = "release-sidecar";
    let directory = scratch.0.join(directory_name);
    let executed = scratch.0.join("candidate-executed");
    let jq_calls = scratch.0.join("jq-called");
    let qualification_calls = scratch.0.join("qualification-called");
    let (mock_bin, real_jq) = install_jq_probe(&scratch)?;
    let (source_sha, git_dir) = source_identity(&root)?;
    let digest = prepare_candidate_fixture(&scratch, directory_name, &source_sha)?;
    prepare_positive_release_fixture(&scratch)?;
    let validation = ReleaseValidation {
        scratch: &scratch,
        mock_bin: &mock_bin,
        real_jq: &real_jq,
        git_dir: &git_dir,
        source_sha: &source_sha,
        executed: &executed,
        jq_calls: &jq_calls,
        qualification_calls: &qualification_calls,
    };
    let positive = validation.run(directory_name)?;
    assert!(
        positive.status.success(),
        "valid sidecar control failed: {}",
        String::from_utf8_lossy(&positive.stderr)
    );
    assert_eq!(fs::read_to_string(&executed)?, "executed\n");
    assert_eq!(
        fs::metadata(directory.join(LINUX.binary))?
            .permissions()
            .mode()
            & 0o777,
        0o500
    );
    assert_eq!(fs::read_to_string(&jq_calls)?, "called\n");
    assert_eq!(fs::read_to_string(&qualification_calls)?, "reached\n");

    fs::remove_file(&executed)?;
    fs::remove_file(&jq_calls)?;
    fs::remove_file(&qualification_calls)?;
    fs::write(
        directory.join(LINUX.sidecar),
        format!("{digest}  unrelated-binary\n"),
    )?;
    let negative = validation.run(directory_name)?;
    assert!(
        !negative.status.success(),
        "accepted a sidecar for another filename"
    );
    assert!(
        !jq_calls.exists(),
        "provenance check ran before filename validation"
    );
    assert!(
        !executed.exists(),
        "candidate ran after a wrong checksum filename"
    );
    assert!(
        !qualification_calls.exists(),
        "qualification ran after a wrong checksum filename"
    );
    Ok(())
}

#[test]
fn authority_provenance_uses_validated_double_quoted_toolchain_inputs() {
    let script = verify_authority_provenance_script(LINUX);
    assert!(script.contains("$VELNOR_RELEASE_RUST_VERSION =~ $semver"));
    assert!(script.contains("$VELNOR_RELEASE_MR_BOXINGTON_VERSION =~ $semver"));
    assert!(script.contains("--arg rust \"$VELNOR_RELEASE_RUST_VERSION\""));
    assert!(script.contains("--arg mr_boxington \"$VELNOR_RELEASE_MR_BOXINGTON_VERSION\""));
    assert!(!script.contains("--arg rust '"));
    assert!(!script.contains("--arg mr_boxington '"));
}

#[test]
fn authority_provenance_rejects_each_changed_binding_and_shell_metacharacters()
-> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let scratch = Scratch::new()?;
    let directory_name = LINUX.directory;
    let directory = scratch.0.join(directory_name);
    let (source_sha, _) = source_identity(&root)?;
    let digest = prepare_candidate_fixture(&scratch, directory_name, &source_sha)?;
    let (mock_bin, real_jq) = install_jq_probe(&scratch)?;
    let provenance_path = directory.join(LINUX.provenance);
    let original = fs::read_to_string(&provenance_path)?;
    let valid = run_authority_validation(&scratch, &mock_bin, &real_jq, &source_sha)?;
    assert!(
        valid.status.success(),
        "valid authority provenance failed: {}",
        String::from_utf8_lossy(&valid.stderr)
    );

    for (old, new) in [
        (source_sha.clone(), "f".repeat(40)),
        (
            LINUX.target.triple().to_owned(),
            "changed-target".to_owned(),
        ),
        ("1.99.1".to_owned(), "1.99.2".to_owned()),
        ("1.26.0".to_owned(), "1.26.1".to_owned()),
        (digest, "0".repeat(64)),
    ] {
        let changed = original.replacen(&old, &new, 1);
        assert_ne!(changed, original, "fixture must contain {old}");
        fs::write(&provenance_path, changed)?;
        let rejected = run_authority_validation(&scratch, &mock_bin, &real_jq, &source_sha)?;
        assert!(
            !rejected.status.success(),
            "accepted changed provenance field {old}"
        );
    }
    fs::write(&provenance_path, original)?;

    let marker = scratch.0.join("shell-injected");
    let mut command = authority_command(&scratch, &mock_bin, &real_jq, &source_sha)?;
    command.env(
        "VELNOR_RELEASE_RUST_VERSION",
        format!("1.99.1; touch {}", marker.display()),
    );
    let rejected = command.output()?;
    assert!(
        !rejected.status.success(),
        "accepted shell syntax as a toolchain version"
    );
    assert!(!marker.exists(), "toolchain input executed shell syntax");
    Ok(())
}

#[test]
fn malformed_authority_versions_stop_before_reading_provenance() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let scratch = Scratch::new()?;
    let (source_sha, _) = source_identity(&root)?;
    prepare_candidate_fixture(&scratch, LINUX.directory, &source_sha)?;
    let (mock_bin, real_jq) = install_jq_probe(&scratch)?;
    for (key, version) in [
        ("VELNOR_RELEASE_RUST_VERSION", "01.99.1"),
        ("VELNOR_RELEASE_MR_BOXINGTON_VERSION", "01.26.0"),
        ("VELNOR_RELEASE_RUST_VERSION", "1.99.1; exit 0"),
        ("VELNOR_RELEASE_MR_BOXINGTON_VERSION", "1.26.0; exit 0"),
    ] {
        let mut command = authority_command(&scratch, &mock_bin, &real_jq, &source_sha)?;
        command.env(key, version);
        assert!(!command.output()?.status.success(), "accepted {key}");
        assert!(
            !scratch.0.join("authority-jq-called").exists(),
            "malformed {key} reached provenance reader"
        );
    }
    Ok(())
}

fn run_authority_validation(
    scratch: &Scratch,
    mock_bin: &Path,
    real_jq: &str,
    source_sha: &str,
) -> Result<std::process::Output, Box<dyn Error>> {
    Ok(authority_command(scratch, mock_bin, real_jq, source_sha)?.output()?)
}

fn authority_command(
    scratch: &Scratch,
    mock_bin: &Path,
    real_jq: &str,
    source_sha: &str,
) -> Result<Command, Box<dyn Error>> {
    let script = verify_authority_provenance_script(LINUX);
    let mut command = run_step(&script, &scratch.0, mock_bin, TOKEN_SENTINEL)?;
    command
        .env("REAL_JQ", real_jq)
        .env("JQ_CALLS", scratch.0.join("authority-jq-called"))
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_SHA", source_sha)
        .env("VELNOR_RELEASE_RUST_VERSION", "1.99.1")
        .env("VELNOR_RELEASE_MR_BOXINGTON_VERSION", "1.26.0");
    Ok(command)
}

fn source_identity(root: &Path) -> Result<(String, String), Box<dyn Error>> {
    let sha = git_fixture::command(root)?
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !sha.status.success() {
        return Err("cannot read candidate source SHA".into());
    }
    let source_sha = String::from_utf8(sha.stdout)?.trim().to_owned();
    let git_dir = git_fixture::command(root)?
        .args(["rev-parse", "--absolute-git-dir"])
        .output()?;
    if !git_dir.status.success() {
        return Err("cannot read candidate source Git directory".into());
    }
    Ok((
        source_sha,
        String::from_utf8(git_dir.stdout)?.trim().to_owned(),
    ))
}

fn prepare_candidate_fixture(
    scratch: &Scratch,
    directory_name: &str,
    source_sha: &str,
) -> Result<String, Box<dyn Error>> {
    let directory = scratch.0.join(directory_name);
    fs::create_dir(&directory)?;
    let candidate = directory.join(LINUX.binary);
    fs::write(
        &candidate,
        format!(
            "#!/bin/sh\nprintf '%s\\n' executed >> \"$CANDIDATE_EXECUTED\"\nprintf '%s\\n' 'velnor-actions {VERSION}'\n"
        ),
    )?;
    let mut permissions = fs::metadata(&candidate)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&candidate, permissions)?;
    let digest = sha256_file(&candidate)?;
    fs::write(
        directory.join(LINUX.sidecar),
        format!("{digest}  {}\n", LINUX.binary),
    )?;
    fs::write(
        directory.join(LINUX.provenance),
        provenance_fixture(&digest, source_sha),
    )?;
    Ok(digest)
}

fn prepare_positive_release_fixture(scratch: &Scratch) -> Result<(), Box<dyn Error>> {
    let scripts = scratch.0.join("scripts");
    let manifests = scratch.0.join("manifest-assets");
    fs::create_dir(&scripts)?;
    fs::create_dir(&manifests)?;
    fs::write(manifests.join("release-manifest.json"), "{}\n")?;
    write_executable(
        &scripts.join("capture-opentofu-goldens.sh"),
        "#!/bin/sh\ntest \"$1\" = check-release && test -x \"$2\" && test -f \"$3\" && test -n \"$4\" || exit 1\nprintf 'reached\\n' >> \"$QUALIFICATION_CALLS\"\n",
    )?;
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err("cannot hash candidate fixture".into());
    }
    String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| "candidate hash output is empty".into())
}

fn provenance_fixture(digest: &str, source_sha: &str) -> String {
    format!(
        r#"{{"schema":1,"version":"{VERSION}","repository":"tailrocks/velnor-new","commit":"{source_sha}","target":"{}","asset":"{}","sha256":"{digest}","toolchain":{{"rust":"1.99.1","mr-boxington":"1.26.0"}}}}"#,
        LINUX.target.triple(),
        LINUX.binary
    )
}

fn release_validation_command(directory_name: &str) -> String {
    format!(
        "{}\n{}",
        verify_provenance_in_directory(LINUX, directory_name, "1.99.1", "1.26.0"),
        qualification_script(LINUX.binary, directory_name)
    )
}

fn install_jq_probe(scratch: &Scratch) -> Result<(PathBuf, String), Box<dyn Error>> {
    let output = Command::new("sh").args(["-c", "command -v jq"]).output()?;
    if !output.status.success() {
        return Err("jq is unavailable for provenance verification".into());
    }
    let real_jq = String::from_utf8(output.stdout)?.trim().to_owned();
    let mock_bin = scratch.0.join("mock-bin");
    fs::create_dir(&mock_bin)?;
    write_executable(
        &mock_bin.join("jq"),
        r#"#!/bin/sh
printf 'called\n' >> "$JQ_CALLS"
exec "$REAL_JQ" "$@"
"#,
    )?;
    Ok((mock_bin, real_jq))
}

struct ReleaseValidation<'a> {
    scratch: &'a Scratch,
    mock_bin: &'a Path,
    real_jq: &'a str,
    git_dir: &'a str,
    source_sha: &'a str,
    executed: &'a Path,
    jq_calls: &'a Path,
    qualification_calls: &'a Path,
}

impl ReleaseValidation<'_> {
    fn run(&self, directory_name: &str) -> Result<std::process::Output, Box<dyn Error>> {
        let script = release_validation_command(directory_name);
        let mut command = run_step(&script, &self.scratch.0, self.mock_bin, TOKEN_SENTINEL)?;
        command
            .env("REAL_JQ", self.real_jq)
            .env("JQ_CALLS", self.jq_calls)
            .env("GIT_DIR", self.git_dir)
            .env("GIT_WORK_TREE", &self.scratch.0)
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env("GITHUB_SHA", self.source_sha)
            .env("GITHUB_WORKFLOW_SHA", self.source_sha)
            .env("GITHUB_WORKSPACE", &self.scratch.0)
            .env("VELNOR_RELEASE_MANIFEST_SHA256", "a".repeat(64))
            .env("CANDIDATE_EXECUTED", self.executed)
            .env("QUALIFICATION_CALLS", self.qualification_calls);
        Ok(command.output()?)
    }
}
