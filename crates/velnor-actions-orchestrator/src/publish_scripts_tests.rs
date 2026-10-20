//! Behavioral cases: the fixed scripts run for real against fixtures.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;
use velnor_actions_contract::config::VersionSource;

use velnor_actions_workflow_renderer::publish_scripts::{
    branch_push_script, inline_version_script, manifest_program, pack_script, parse_tag_script,
    required_wait_script,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn sh(script: &str, dir: &Path, env: &[(&str, &str)]) -> std::io::Result<Output> {
    Command::new("sh")
        .args(["-c", script])
        .current_dir(dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin")
        .envs(env.iter().copied())
        .output()
}

fn parse(tag: &str) -> Result<(bool, String), Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let out = dir.path().join("out");
    fs::write(&out, "")?;
    let script = parse_tag_script("acme/widgets", &["demo-schema", "ui"]);
    let output = sh(
        &script,
        dir.path(),
        &[
            ("GITHUB_EVENT_NAME", "push"),
            ("GITHUB_REF_TYPE", "tag"),
            ("GITHUB_REPOSITORY", "acme/widgets"),
            ("GITHUB_REF_NAME", tag),
            ("GITHUB_OUTPUT", out.to_str().ok_or("path")?),
        ],
    )?;
    Ok((output.status.success(), fs::read_to_string(out)?))
}

#[test]
fn declared_tags_parse_into_outputs() -> TestResult {
    assert_eq!(
        parse("demo-schema-v1.4.0")?,
        (true, "artifact=demo-schema\nversion=1.4.0\n".to_owned())
    );
    assert_eq!(
        parse("ui-v10.0.7")?,
        (true, "artifact=ui\nversion=10.0.7\n".to_owned())
    );
    Ok(())
}

#[test]
fn undeclared_or_malformed_tags_fail_closed() -> TestResult {
    for tag in [
        "other-v1.0.0",
        "ui-v1.0",
        "ui-v01.0.0",
        "ui-v1.0.0-rc1",
        "UI-v1.0.0",
        "ui-v1.0.0.0",
        "ui-v1.0.x",
        "demo-v1.0.0",
        "ui-v1.0.0;id",
        "-v1.0.0",
    ] {
        let (ok, written) = parse(tag)?;
        assert!(!ok && written.is_empty(), "{tag}");
    }
    Ok(())
}

#[test]
fn wrong_event_ref_type_or_repository_fails() -> TestResult {
    let dir = TempDir::new()?;
    let out = dir.path().join("out");
    fs::write(&out, "")?;
    let script = parse_tag_script("acme/widgets", &["ui"]);
    let base = [
        ("GITHUB_EVENT_NAME", "push"),
        ("GITHUB_REF_TYPE", "tag"),
        ("GITHUB_REPOSITORY", "acme/widgets"),
        ("GITHUB_REF_NAME", "ui-v1.0.0"),
        ("GITHUB_OUTPUT", out.to_str().ok_or("path")?),
    ];
    assert!(sh(&script, dir.path(), &base)?.status.success());
    for (key, value) in [
        ("GITHUB_EVENT_NAME", "workflow_dispatch"),
        ("GITHUB_REF_TYPE", "branch"),
        ("GITHUB_REPOSITORY", "evil/widgets"),
    ] {
        let env: Vec<(&str, &str)> = base
            .iter()
            .map(|(k, v)| if *k == key { (*k, value) } else { (*k, *v) })
            .collect();
        assert!(!sh(&script, dir.path(), &env)?.status.success(), "{key}");
    }
    Ok(())
}

#[test]
fn inline_version_must_match_the_tag() -> TestResult {
    let dir = TempDir::new()?;
    let script = inline_version_script("3.1.0");
    assert!(
        sh(&script, dir.path(), &[("GITHUB_REF_NAME", "x-v3.1.0")])?
            .status
            .success()
    );
    assert!(
        !sh(&script, dir.path(), &[("GITHUB_REF_NAME", "x-v3.1.1")])?
            .status
            .success()
    );
    Ok(())
}

/// GNU tar is required for the deterministic flags; skip on BSD tar.
fn gnu_tar() -> bool {
    Command::new("tar")
        .arg("--version")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("GNU tar"))
}

fn pack(root: &Path, mtime_bump: bool) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let out = TempDir::new()?;
    if mtime_bump {
        let file = root.join("data/a.txt");
        let text = fs::read(&file)?;
        fs::write(&file, text)?;
    }
    let script = pack_script("demo", &["data".to_owned()]);
    let temp = out.path().to_str().ok_or("path")?;
    let output = sh(
        &script,
        root,
        &[("GITHUB_REF_NAME", "demo-v1.2.3"), ("RUNNER_TEMP", temp)],
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let sidecar = fs::read_to_string(out.path().join("bundle/demo-1.2.3.tar.gz.sha256"))?;
    assert!(
        sidecar.trim_end().ends_with("  demo-1.2.3.tar.gz"),
        "{sidecar}"
    );
    Ok(fs::read(out.path().join("bundle/demo-1.2.3.tar.gz"))?)
}

#[test]
fn bundles_are_byte_identical_across_builds() -> TestResult {
    if !gnu_tar() {
        return Ok(());
    }
    let repo = TempDir::new()?;
    fs::create_dir_all(repo.path().join("data/sub"))?;
    fs::write(repo.path().join("data/a.txt"), "a\n")?;
    fs::write(repo.path().join("data/sub/z.txt"), "z\n")?;
    let first = pack(repo.path(), false)?;
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let second = pack(repo.path(), true)?;
    assert_eq!(first, second, "mtime must not leak into the bundle");
    Ok(())
}

#[test]
fn symlinks_and_missing_paths_fail_the_pack() -> TestResult {
    if !gnu_tar() {
        return Ok(());
    }
    let repo = TempDir::new()?;
    fs::create_dir_all(repo.path().join("data"))?;
    std::os::unix::fs::symlink("/etc/passwd", repo.path().join("data/link"))?;
    let out = TempDir::new()?;
    let temp = out.path().to_str().ok_or("path")?;
    let env = [("GITHUB_REF_NAME", "demo-v1.2.3"), ("RUNNER_TEMP", temp)];
    assert!(
        !sh(
            &pack_script("demo", &["data".to_owned()]),
            repo.path(),
            &env
        )?
        .status
        .success()
    );
    assert!(
        !sh(
            &pack_script("demo", &["nope".to_owned()]),
            repo.path(),
            &env
        )?
        .status
        .success()
    );
    Ok(())
}

#[test]
fn manifest_programs_match_the_rust_reader() -> TestResult {
    if Command::new("jq").arg("--version").output().is_err() {
        return Ok(());
    }
    let cases: [(VersionSource, &str, &str); 6] = [
        (
            VersionSource::CargoToml,
            "[workspace.package]\nversion = \"1.2.3\"\n",
            "1.2.3",
        ),
        (
            VersionSource::CargoToml,
            "[package]\nversion.workspace = true\n",
            "1.2.3",
        ),
        (
            VersionSource::CargoToml,
            "[dependencies]\nversion = \"1.2.3\"\n",
            "1.2.3",
        ),
        (
            VersionSource::GradleProperties,
            "a=1\nversion=2.3.0\n",
            "2.3.0",
        ),
        (
            VersionSource::GradleProperties,
            "version=2.3.0-SNAPSHOT\n",
            "2.3.0",
        ),
        (
            VersionSource::PackageJson,
            "{\"version\":\"1.4.0\"}",
            "1.4.0",
        ),
    ];
    for (source, text, tag_version) in cases {
        let rust_matches = source.read(text).as_deref() == Some(tag_version);
        let doc = serde_json::json!({ "content": base64_of(text) }).to_string();
        let mut child = Command::new("jq")
            .args(["-e", &manifest_program(source)])
            .env("VELNOR_TAG_VERSION", tag_version)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        std::io::Write::write_all(&mut child.stdin.take().ok_or("stdin")?, doc.as_bytes())?;
        // `empty` output is the success shape, so `jq -e` exits 4, not 0.
        let jq_matches = child.wait()?.code() == Some(4);
        assert_eq!(rust_matches, jq_matches, "{source:?}: {text}");
    }
    Ok(())
}

fn base64_of(text: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(text)
}

fn has_jq() -> bool {
    Command::new("jq").arg("--version").output().is_ok()
}

/// Run the wait script against a fake `gh` that applies the real jq program
/// to a canned check-run document.
fn wait(runs: &str, attempts: u16) -> Result<bool, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let doc = dir.path().join("runs.json");
    fs::write(&doc, format!("{{\"check_runs\":{runs}}}"))?;
    let fake = "gh() { jq -r \"$4\" < \"$FAKE_RUNS\"; }";
    let script = required_wait_script(fake, attempts, 0);
    let temp = dir.path().to_str().ok_or("path")?;
    let env = [
        ("RUNNER_TEMP", temp),
        ("GITHUB_REPOSITORY", "acme/widgets"),
        ("GITHUB_SHA", "abc"),
        ("FAKE_RUNS", doc.to_str().ok_or("path")?),
    ];
    Ok(sh(&script, dir.path(), &env)?.status.success())
}

#[test]
fn the_required_wait_settles_on_the_newest_check_run() -> TestResult {
    if !has_jq() {
        return Ok(());
    }
    let run = |id: u32, status: &str, conclusion: &str, app: &str| {
        format!(
            "{{\"id\":{id},\"name\":\"Required\",\"status\":\"{status}\",\"conclusion\":\"{conclusion}\",\"app\":{{\"slug\":\"{app}\"}}}}"
        )
    };
    let green = format!("[{}]", run(2, "completed", "success", "github-actions"));
    assert!(wait(&green, 3)?);
    let rerun = format!(
        "[{},{}]",
        run(1, "completed", "failure", "github-actions"),
        run(2, "completed", "success", "github-actions")
    );
    assert!(
        wait(&rerun, 3)?,
        "a successful re-run supersedes the failure"
    );
    let red = format!("[{}]", run(2, "completed", "failure", "github-actions"));
    assert!(!wait(&red, 3)?);
    let spoofed = format!("[{}]", run(2, "completed", "success", "evil-app"));
    assert!(!wait(&spoofed, 2)?, "only the GitHub Actions check counts");
    let running = format!("[{}]", run(2, "in_progress", "", "github-actions"));
    assert!(
        !wait(&running, 2)?,
        "an unsettled verdict exhausts the budget"
    );
    assert!(!wait("[]", 2)?, "no verdict yet exhausts the budget");
    Ok(())
}

#[test]
fn branch_push_gate_requires_the_exact_ref() -> TestResult {
    let dir = TempDir::new()?;
    let script = branch_push_script("acme/widgets", "main");
    let base = [
        ("GITHUB_EVENT_NAME", "push"),
        ("GITHUB_REF_TYPE", "branch"),
        ("GITHUB_REPOSITORY", "acme/widgets"),
        ("GITHUB_REF", "refs/heads/main"),
    ];
    assert!(sh(&script, dir.path(), &base)?.status.success());
    for (key, value) in [
        ("GITHUB_EVENT_NAME", "workflow_dispatch"),
        ("GITHUB_REF_TYPE", "tag"),
        ("GITHUB_REPOSITORY", "evil/widgets"),
        ("GITHUB_REF", "refs/heads/other"),
    ] {
        let env: Vec<(&str, &str)> = base
            .iter()
            .map(|(k, v)| if *k == key { (*k, value) } else { (*k, *v) })
            .collect();
        assert!(!sh(&script, dir.path(), &env)?.status.success(), "{key}");
    }
    Ok(())
}
