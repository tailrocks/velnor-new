//! Behavioral cases: the Maven bundle scripts run for real against fixture zips.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;
use velnor_actions_workflow_renderer::publish_scripts::maven::{
    collect_script, upload_script, verify_script,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const OUTPUT_DIR: &str = "backend/build/maven-bundle";

fn tools_present() -> bool {
    ["zip", "unzip", "sha256sum"].iter().all(|tool| {
        Command::new(tool)
            .arg("-h")
            .output()
            .is_ok_and(|out| out.status.code().is_some())
    })
}

fn sh(script: &str, workspace: &Path, temp: &Path, tag: &str) -> std::io::Result<Output> {
    Command::new("sh")
        .args(["-c", script])
        .current_dir(workspace)
        .env_clear()
        .env(
            "PATH",
            "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin:/sbin",
        )
        .env("GITHUB_WORKSPACE", workspace)
        .env("RUNNER_TEMP", temp)
        .env("GITHUB_REF_NAME", tag)
        .output()
}

/// Build `<id>-<version>-maven.zip` holding `entries` plus the producer's sidecar.
fn bundle(workspace: &Path, name: &str, entries: &[&str]) -> TestResult {
    let dir = workspace.join(OUTPUT_DIR);
    let tree = workspace.join("tree");
    for entry in entries {
        let path = tree.join(entry);
        fs::create_dir_all(path.parent().ok_or("parent")?)?;
        fs::write(path, "x\n")?;
    }
    fs::create_dir_all(&dir)?;
    let status = Command::new("zip")
        .arg("-qr")
        .arg(dir.join(name))
        .arg(".")
        .current_dir(&tree)
        .status()?;
    assert!(status.success());
    let sum = Command::new("sha256sum")
        .arg(name)
        .current_dir(&dir)
        .output()?;
    fs::write(dir.join(format!("{name}.sha256")), sum.stdout)?;
    Ok(())
}

const POM: &str = "repository/com/chainargos/libs/1.0.0/libs-1.0.0.pom";

#[test]
fn a_producer_bundle_is_verified_and_moved_into_the_bundle_directory() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let work = TempDir::new()?;
    let temp = TempDir::new()?;
    bundle(work.path(), "libs-1.0.0-maven.zip", &[POM])?;
    let output = sh(
        &collect_script("libs", OUTPUT_DIR),
        work.path(),
        temp.path(),
        "libs-v1.0.0",
    )?;
    assert!(output.status.success(), "{output:?}");
    let moved = temp.path().join("bundle");
    let mut names: Vec<_> = fs::read_dir(&moved)?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_, _>>()?;
    names.sort();
    assert_eq!(
        names,
        ["libs-1.0.0-maven.zip", "libs-1.0.0-maven.zip.sha256"]
    );
    let verified = sh(
        &verify_script("libs"),
        work.path(),
        temp.path(),
        "libs-v1.0.0",
    )?;
    assert!(verified.status.success(), "{verified:?}");
    fs::write(moved.join("libs-1.0.0-maven.zip"), "tampered")?;
    let tampered = sh(
        &verify_script("libs"),
        work.path(),
        temp.path(),
        "libs-v1.0.0",
    )?;
    assert!(!tampered.status.success());
    Ok(())
}

#[test]
fn a_bundle_that_disagrees_with_the_tag_or_its_checksum_fails_closed() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let cases: [(&str, &[&str], &str, &str); 4] = [
        (
            "wrong tag version",
            &[POM],
            "libs-1.0.0-maven.zip",
            "libs-v1.0.1",
        ),
        (
            "pom at another version",
            &["repository/com/chainargos/libs/0.9.0/libs-0.9.0.pom"],
            "libs-1.0.0-maven.zip",
            "libs-v1.0.0",
        ),
        (
            "entry outside repository/",
            &[POM, "stray.txt"],
            "libs-1.0.0-maven.zip",
            "libs-v1.0.0",
        ),
        (
            "no pom",
            &["repository/readme.txt"],
            "libs-1.0.0-maven.zip",
            "libs-v1.0.0",
        ),
    ];
    for (label, entries, name, tag) in cases {
        let work = TempDir::new()?;
        let temp = TempDir::new()?;
        bundle(work.path(), name, entries)?;
        let output = sh(
            &collect_script("libs", OUTPUT_DIR),
            work.path(),
            temp.path(),
            tag,
        )?;
        assert!(!output.status.success(), "{label}: {output:?}");
        assert!(
            !temp.path().join("bundle/libs-1.0.0-maven.zip").exists(),
            "{label}: nothing moves on failure"
        );
    }
    let work = TempDir::new()?;
    let temp = TempDir::new()?;
    bundle(work.path(), "libs-1.0.0-maven.zip", &[POM])?;
    fs::write(
        work.path()
            .join(OUTPUT_DIR)
            .join("libs-1.0.0-maven.zip.sha256"),
        format!("{}  libs-1.0.0-maven.zip\n", "0".repeat(64)),
    )?;
    let bad_sum = sh(
        &collect_script("libs", OUTPUT_DIR),
        work.path(),
        temp.path(),
        "libs-v1.0.0",
    )?;
    assert!(!bad_sum.status.success());
    Ok(())
}

#[test]
fn the_upload_never_overwrites_an_existing_asset() {
    let script = upload_script("gh() { :; }", "libs");
    assert!(script.contains("libs-$ver-maven.zip"));
    assert!(script.contains("libs-$ver-maven.zip.sha256"));
    assert!(script.contains("--verify-tag"));
    assert!(!script.contains("--clobber"));
}

/// A fake `gh` over a directory standing in for the release's assets.
const FAKE_GH: &str = r#"gh() { case "$1 $2" in
  "release view") case "$*" in *--json*) ls "$FAKE_RELEASE" ;; *) test -d "$FAKE_RELEASE" ;; esac ;;
  "release download") shift 2; pat=; dir=; while [ $# -gt 0 ]; do case "$1" in --pattern) pat=$2; shift ;; --dir) dir=$2; shift ;; esac; shift; done; cp "$FAKE_RELEASE/$pat" "$dir/$pat" ;;
  "release upload") shift 3; shift 2; for f in "$@"; do cp "$f" "$FAKE_RELEASE/"; done ;;
  "release create") echo created >&2; exit 9 ;;
esac; }"#;

fn run_upload(release: &Path, temp: &Path) -> std::io::Result<Output> {
    Command::new("sh")
        .args(["-c", &upload_script(FAKE_GH, "libs")])
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("RUNNER_TEMP", temp)
        .env("GITHUB_REF_NAME", "libs-v1.0.0")
        .env("GITHUB_REPOSITORY", "acme/widgets")
        .env("FAKE_RELEASE", release)
        .output()
}

#[test]
fn an_existing_identical_asset_is_skipped_and_a_different_one_refused() -> TestResult {
    if !tools_present() {
        return Ok(());
    }
    let temp = TempDir::new()?;
    let release = TempDir::new()?;
    let bundle = temp.path().join("bundle");
    fs::create_dir_all(&bundle)?;
    for name in ["libs-1.0.0-maven.zip", "libs-1.0.0-maven.zip.sha256"] {
        fs::write(bundle.join(name), name)?;
        fs::write(release.path().join(name), name)?;
    }
    let same = run_upload(release.path(), temp.path())?;
    assert!(same.status.success(), "{same:?}");
    assert!(String::from_utf8_lossy(&same.stdout).contains("same sha256, skipping"));
    fs::write(release.path().join("libs-1.0.0-maven.zip"), "manual upload")?;
    let different = run_upload(release.path(), temp.path())?;
    assert!(!different.status.success());
    assert!(String::from_utf8_lossy(&different.stderr).contains("refusing to overwrite"));
    assert_eq!(
        fs::read_to_string(release.path().join("libs-1.0.0-maven.zip"))?,
        "manual upload"
    );
    fs::remove_file(release.path().join("libs-1.0.0-maven.zip.sha256"))?;
    fs::write(
        release.path().join("libs-1.0.0-maven.zip"),
        "libs-1.0.0-maven.zip",
    )?;
    let partial = run_upload(release.path(), temp.path())?;
    assert!(partial.status.success(), "{partial:?}");
    assert!(release.path().join("libs-1.0.0-maven.zip.sha256").exists());
    Ok(())
}
