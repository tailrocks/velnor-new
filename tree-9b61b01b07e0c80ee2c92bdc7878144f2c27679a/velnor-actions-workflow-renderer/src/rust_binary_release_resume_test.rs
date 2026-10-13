use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{RenderedCommands, scripts};
use crate::tool_seed_test_support::git_fixture;
use velnor_actions_contract::RustBinaryReleaseConfig;

#[path = "rust_binary_release_resume_mock.rs"]
mod mock;
use mock::MOCK_GH;

const REPOSITORY: &str = "owner/demo";
const PACKAGE: &str = "demo-package";
const BINARY: &str = "demo-binary";
const RELEASE_ID: &str = "987";

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!(
            "velnor-binary-release-resume-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        Ok(Self(root))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

struct ReleaseFixture {
    root: PathBuf,
    repository: PathBuf,
    bin: PathBuf,
    state: PathBuf,
    log: PathBuf,
    output: PathBuf,
    source_sha: String,
    tag: String,
}

impl ReleaseFixture {
    fn new(root: &Path) -> Result<Self, Box<dyn Error>> {
        let repository = root.join("repository");
        let source_sha = create_tagged_repository(&repository)?;
        let bin = root.join("bin");
        install_mock_gh(&bin.join("gh"))?;
        Ok(Self {
            root: root.to_path_buf(),
            repository,
            bin,
            state: root.join("release.json"),
            log: root.join("gh.log"),
            output: root.join("github-output"),
            source_sha,
            tag: format!("{PACKAGE}-v1.0.0"),
        })
    }

    fn publish(&self, script: &str, resume_id: &str) -> Result<Output, Box<dyn Error>> {
        run_publish(
            script,
            &self.root,
            &self.bin,
            &self.state,
            &self.log,
            &self.source_sha,
            &self.tag,
            resume_id,
        )
    }

    fn select_draft(&self, script: &str) -> Result<(), Box<dyn Error>> {
        fs::write(&self.output, "")?;
        let output = run_resolver(
            script,
            &self.repository,
            &self.root,
            &self.bin,
            &self.output,
            &self.source_sha,
            &self.state,
            &self.log,
        )?;
        assert!(output.status.success(), "{}", stderr(&output));
        let outputs = fs::read_to_string(&self.output)?;
        assert!(
            outputs.contains(&format!("tag={}\n", self.tag)),
            "{outputs}"
        );
        assert!(
            outputs.contains(&format!("resume_release_id={RELEASE_ID}\n")),
            "{outputs}"
        );
        Ok(())
    }

    fn reject_mismatched_assets(&self, script: &str) -> Result<(), Box<dyn Error>> {
        let original = fs::read(&self.state)?;
        rewrite_json(&self.state, ".assets[0].digest = \"sha256:bad-digest\"")?;
        let output = self.publish(script, RELEASE_ID)?;
        assert!(!output.status.success());
        assert!(stderr(&output).contains("unexpected or mismatched assets"));
        assert_eq!(fs::read_to_string(&self.log)?.matches("upload:").count(), 0);
        fs::write(&self.state, original)?;
        Ok(())
    }
}

#[test]
fn interrupted_create_draft_is_selected_and_resumed_with_verified_assets()
-> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let fixture = ReleaseFixture::new(&scratch.0)?;
    let commands = mock_commands();
    let prep_script = scripts::prepare_assets(PACKAGE, BINARY);
    prepare_archives(&scratch.0)?;
    let prepared = run_prepare(&prep_script, &scratch.0)?;
    assert!(prepared.status.success(), "{}", stderr(&prepared));

    let publish_script = scripts::publish(PACKAGE, BINARY, "gh");
    // The mock leaves a partial draft before failing, modeling interruption or failed cleanup.
    let first = fixture.publish(&publish_script, "")?;
    assert!(
        !first.status.success(),
        "interrupted create unexpectedly succeeded"
    );
    assert!(stderr(&first).contains("leftover draft will be checked on the next run"));
    assert!(fixture.state.is_file(), "mock create left no draft behind");
    let verify_script = scripts::verify_source(&config(), BINARY, &commands);
    fixture.select_draft(&verify_script)?;
    fixture.reject_mismatched_assets(&publish_script)?;

    let resumed = fixture.publish(&publish_script, RELEASE_ID)?;
    assert!(resumed.status.success(), "{}", stderr(&resumed));
    assert_json(
        &fixture.state,
        ".draft == false and (.assets | length) == 3",
    )?;
    let calls = fs::read_to_string(fixture.log)?;
    assert_eq!(calls.matches("create-left-partial-draft").count(), 1);
    assert_eq!(calls.matches("upload:").count(), 2, "{calls}");
    assert_eq!(calls.matches("patch").count(), 1, "{calls}");
    Ok(())
}

fn create_tagged_repository(root: &Path) -> Result<String, Box<dyn Error>> {
    fs::create_dir_all(root)?;
    git(root, &["init", "--quiet", "--initial-branch=main"])?;
    git(root, &["config", "user.name", "Binary Release Test"])?;
    git(
        root,
        &["config", "user.email", "binary-release@example.invalid"],
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='demo-package'\nversion='1.0.0'\n",
    )?;
    git(root, &["add", "Cargo.toml"])?;
    git(root, &["commit", "--quiet", "-m", "release source"])?;
    git(root, &["tag", &format!("{PACKAGE}-v1.0.0")])?;
    Ok(
        String::from_utf8(git(root, &["rev-parse", "HEAD"])?.stdout)?
            .trim()
            .to_owned(),
    )
}

fn git(root: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    let output = git_fixture::command(root)?.args(args).output()?;
    if !output.status.success() {
        return Err(format!("git failed: {}", stderr(&output)).into());
    }
    Ok(output)
}

fn prepare_archives(root: &Path) -> Result<(), Box<dyn Error>> {
    for (directory, target) in [
        ("incoming-linux", "x86_64-unknown-linux-gnu"),
        ("incoming-macos", "aarch64-apple-darwin"),
    ] {
        let incoming = root.join("assets").join(directory);
        let source = root.join(format!("source-{directory}"));
        fs::create_dir_all(&incoming)?;
        fs::create_dir_all(&source)?;
        let binary = source.join(BINARY);
        fs::write(&binary, b"test binary")?;
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
        let archive = incoming.join(format!("{BINARY}-1.0.0-{target}.tar.gz"));
        let status = Command::new("tar")
            .args(["-czf"])
            .arg(archive)
            .args(["-C"])
            .arg(source)
            .arg(BINARY)
            .status()?;
        if !status.success() {
            return Err("failed to create archive fixture".into());
        }
    }
    Ok(())
}

fn install_mock_gh(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(path.parent().ok_or("mock bin path has no parent")?)?;
    fs::write(path, MOCK_GH)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn config() -> RustBinaryReleaseConfig {
    RustBinaryReleaseConfig {
        enabled: true,
        manifest_path: "Cargo.toml".to_owned(),
        package: PACKAGE.to_owned(),
        binary: Some(BINARY.to_owned()),
        source_commit_env: None,
    }
}

fn mock_commands() -> RenderedCommands {
    RenderedCommands {
        install_rust: String::new(),
        metadata: "printf '%s\\n' '{\"packages\":[{\"name\":\"demo-package\",\"version\":\"1.0.0\",\"targets\":[{\"name\":\"demo-binary\",\"kind\":[\"bin\"]}]}]}'".to_owned(),
        rustc_version: String::new(),
        rust_version: String::new(),
        build_linux: String::new(),
        build_macos: String::new(),
        install_gh: String::new(),
        gh_prefix: "gh".to_owned(),
    }
}

fn run_prepare(script: &str, root: &Path) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new("bash")
        .args(["-c", script])
        .current_dir(root)
        .env("SOURCE_SHA", "0123456789012345678901234567890123456789")
        .env("RELEASE_TAG", format!("{PACKAGE}-v1.0.0"))
        .env("RELEASE_VERSION", "1.0.0")
        .env_remove("GH_TOKEN")
        .output()?)
}

#[expect(
    clippy::too_many_arguments,
    reason = "publisher inputs are each bound to their workflow environment variable"
)]
fn run_publish(
    script: &str,
    root: &Path,
    bin: &Path,
    state: &Path,
    log: &Path,
    source_sha: &str,
    tag: &str,
    resume_id: &str,
) -> Result<Output, Box<dyn Error>> {
    let path = format!("{}:{}", bin.display(), std::env::var("PATH")?);
    Ok(Command::new("bash")
        .args(["-c", script])
        .current_dir(root)
        .env("PATH", path)
        .env("GH_TOKEN", "write-test-token")
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .env("SOURCE_SHA", source_sha)
        .env("DEFAULT_SHA", source_sha)
        .env("RELEASE_TAG", tag)
        .env("RELEASE_VERSION", "1.0.0")
        .env("RESUME_RELEASE_ID", resume_id)
        .env("MOCK_REPOSITORY", REPOSITORY)
        .env("MOCK_SOURCE_SHA", source_sha)
        .env("MOCK_DEFAULT_SHA", source_sha)
        .env("MOCK_TAG", tag)
        .env("MOCK_RELEASE_ID", RELEASE_ID)
        .env("MOCK_RELEASE_STATE", state)
        .env("MOCK_LOG", log)
        .output()?)
}

#[expect(
    clippy::too_many_arguments,
    reason = "resolver inputs are each bound to their workflow environment variable"
)]
fn run_resolver(
    script: &str,
    repository: &Path,
    runner_temp: &Path,
    bin: &Path,
    github_output: &Path,
    source_sha: &str,
    state: &Path,
    log: &Path,
) -> Result<Output, Box<dyn Error>> {
    let path = format!("{}:{}", bin.display(), std::env::var("PATH")?);
    Ok(Command::new("bash")
        .args(["-c", script])
        .current_dir(repository)
        .env("PATH", path)
        .env("GH_TOKEN", "read-only-test-token")
        .env("GITHUB_EVENT_NAME", "schedule")
        .env("GITHUB_SHA", source_sha)
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REF_NAME", "main")
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .env("MOCK_REPOSITORY", REPOSITORY)
        .env("MOCK_SOURCE_SHA", source_sha)
        .env("MOCK_DEFAULT_SHA", source_sha)
        .env("MOCK_TAG", format!("{PACKAGE}-v1.0.0"))
        .env("MOCK_RELEASE_ID", RELEASE_ID)
        .env("MOCK_RELEASE_STATE", state)
        .env("MOCK_LOG", log)
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_OUTPUT", github_output)
        .output()?)
}

fn assert_json(path: &Path, expression: &str) -> Result<(), Box<dyn Error>> {
    let output = Command::new("jq")
        .args(["-e", expression])
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err(format!("release JSON check failed: {}", stderr(&output)).into());
    }
    Ok(())
}

fn rewrite_json(path: &Path, filter: &str) -> Result<(), Box<dyn Error>> {
    let output = Command::new("jq").arg(filter).arg(path).output()?;
    if !output.status.success() {
        return Err(format!("failed to mutate fixture JSON: {}", stderr(&output)).into());
    }
    fs::write(path, output.stdout)?;
    Ok(())
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
