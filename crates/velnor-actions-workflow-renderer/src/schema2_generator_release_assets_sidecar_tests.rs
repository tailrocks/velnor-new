use super::super::{LINUX, VERSION, qualification_script, verify_provenance_in_directory};
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
        r#"{{"schema":1,"version":"{VERSION}","repository":"tailrocks/velnor-new","commit":"{source_sha}","target":"{}","asset":"{}","sha256":"{digest}","toolchain":{{"rust":"1.99.1","mr-boxington":"1.25.0"}}}}"#,
        LINUX.target.triple(),
        LINUX.binary
    )
}

fn release_validation_command(directory_name: &str) -> String {
    format!(
        "{}\n{}",
        verify_provenance_in_directory(LINUX, directory_name, "1.99.1", "1.25.0"),
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
