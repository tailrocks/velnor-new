use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use super::super::RenderedCommands;
use super::{BINARY, PACKAGE, RELEASE_ID, REPOSITORY, ReleaseFixture, mock::MOCK_GH};
use crate::tool_seed_test_support::git_fixture;
use velnor_actions_contract::RustBinaryReleaseConfig;

pub(super) fn create_tagged_repository(root: &Path) -> Result<String, Box<dyn Error>> {
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

pub(super) fn git(root: &Path, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    let output = git_fixture::command(root)?.args(args).output()?;
    if !output.status.success() {
        return Err(format!("git failed: {}", stderr(&output)).into());
    }
    Ok(output)
}

pub(super) fn prepare_archives(root: &Path, version: &str) -> Result<(), Box<dyn Error>> {
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
        let archive = incoming.join(format!("{BINARY}-{version}-{target}.tar.gz"));
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

pub(super) fn install_mock_gh(path: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(path.parent().ok_or("mock bin path has no parent")?)?;
    fs::write(path, MOCK_GH)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

pub(super) fn config() -> RustBinaryReleaseConfig {
    RustBinaryReleaseConfig {
        enabled: true,
        manifest_path: "Cargo.toml".to_owned(),
        package: PACKAGE.to_owned(),
        binary: Some(BINARY.to_owned()),
        source_commit_env: None,
    }
}

pub(super) fn mock_commands() -> RenderedCommands {
    RenderedCommands {
        install_rust: String::new(),
        metadata: r#"python3 -c 'import json; line=[x for x in open("Cargo.toml") if x.startswith("version=")][0]; version=line.split(chr(61))[1].strip().strip(chr(39)); print(json.dumps({"packages":[{"name":"demo-package","version":version,"targets":[{"name":"demo-binary","kind":["bin"]}]}]}))'"#.to_owned(),
        rustc_version: String::new(),
        rust_version: String::new(),
        build_linux: String::new(),
        build_macos: String::new(),
        install_gh: String::new(),
        gh_prefix: "gh".to_owned(),
    }
}

pub(super) fn run_prepare(
    script: &str,
    root: &Path,
    version: &str,
) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new("bash")
        .args(["-c", script])
        .current_dir(root)
        .env("SOURCE_SHA", "0123456789012345678901234567890123456789")
        .env("RELEASE_TAG", format!("{PACKAGE}-v{version}"))
        .env("RELEASE_VERSION", version)
        .env_remove("GH_TOKEN")
        .output()?)
}

pub(super) fn run_publish(
    script: &str,
    fixture: &ReleaseFixture,
    duplicate_draft_pages: bool,
) -> Result<Output, Box<dyn Error>> {
    let path = format!("{}:{}", fixture.bin.display(), std::env::var("PATH")?);
    let version_prefix = format!("{PACKAGE}-v");
    let version = fixture
        .tag
        .strip_prefix(&version_prefix)
        .ok_or("fixture release tag has the wrong prefix")?;
    Ok(Command::new("bash")
        .args(["-c", script])
        .current_dir(&fixture.root)
        .env("PATH", path)
        .env("GH_TOKEN", "write-test-token")
        .env("GITHUB_REPOSITORY", REPOSITORY)
        .env("SOURCE_SHA", fixture.source_sha.as_str())
        .env("DEFAULT_SHA", fixture.source_sha.as_str())
        .env("RELEASE_TAG", fixture.tag.as_str())
        .env("RELEASE_VERSION", version)
        .env("MOCK_REPOSITORY", REPOSITORY)
        .env("MOCK_SOURCE_SHA", fixture.source_sha.as_str())
        .env("MOCK_DEFAULT_SHA", fixture.source_sha.as_str())
        .env("MOCK_TAG", fixture.tag.as_str())
        .env("MOCK_RELEASE_ID", RELEASE_ID)
        .env("MOCK_RELEASE_STATE", &fixture.state)
        .env("MOCK_LOG", &fixture.log)
        .env(
            "MOCK_DUPLICATE_DRAFT_PAGES",
            if duplicate_draft_pages {
                "true"
            } else {
                "false"
            },
        )
        .output()?)
}

#[expect(
    clippy::too_many_arguments,
    reason = "resolver inputs are each bound to their workflow environment variable"
)]
pub(super) fn run_resolver(
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

pub(super) fn assert_json(path: &Path, expression: &str) -> Result<(), Box<dyn Error>> {
    let output = Command::new("jq")
        .args(["-e", expression])
        .arg(path)
        .output()?;
    if !output.status.success() {
        return Err(format!("release JSON check failed: {}", stderr(&output)).into());
    }
    Ok(())
}

pub(super) fn rewrite_json(path: &Path, filter: &str) -> Result<(), Box<dyn Error>> {
    let output = Command::new("jq").arg(filter).arg(path).output()?;
    if !output.status.success() {
        return Err(format!("failed to mutate fixture JSON: {}", stderr(&output)).into());
    }
    fs::write(path, output.stdout)?;
    Ok(())
}

pub(super) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
