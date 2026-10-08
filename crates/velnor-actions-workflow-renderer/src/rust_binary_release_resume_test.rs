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

    fn publish(&self, script: &str) -> Result<Output, Box<dyn Error>> {
        self.publish_with_duplicate_draft_pages(script, false)
    }

    fn publish_with_duplicate_draft_pages(
        &self,
        script: &str,
        duplicate_draft_pages: bool,
    ) -> Result<Output, Box<dyn Error>> {
        run_publish(script, self, duplicate_draft_pages)
    }

    fn verify_source_with_hidden_draft(&self, script: &str) -> Result<(), Box<dyn Error>> {
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
        assert!(outputs.contains("should_release=true\n"), "{outputs}");
        assert!(!outputs.contains("resume_release_id="), "{outputs}");
        let calls = fs::read_to_string(&self.log)?;
        assert!(calls.contains("release-list-read:hidden-draft"), "{calls}");
        Ok(())
    }

    fn reject_existing_draft(&self, script: &str) -> Result<(), Box<dyn Error>> {
        let original_state = fs::read(&self.state)?;
        let original_log = fs::read_to_string(&self.log)?;
        let output = self.publish(script)?;
        assert!(!output.status.success());
        assert!(stderr(&output).contains("already has a draft release"));
        assert!(stderr(&output).contains("reconcile or remove it manually"));
        assert_eq!(fs::read(&self.state)?, original_state);
        let calls = fs::read_to_string(&self.log)?;
        for operation in [
            "draft-detail:",
            "upload:",
            "patch\n",
            "create-left-partial-draft",
        ] {
            assert_eq!(
                calls.matches(operation).count(),
                original_log.matches(operation).count(),
                "existing draft operation {operation:?} changed: {calls}"
            );
        }
        Ok(())
    }

    fn add_higher_version_tag(&mut self) -> Result<(), Box<dyn Error>> {
        const VERSION: &str = "2.0.0";
        fs::write(
            self.repository.join("Cargo.toml"),
            format!("[package]\nname='{PACKAGE}'\nversion='{VERSION}'\n"),
        )?;
        git(&self.repository, &["add", "Cargo.toml"])?;
        git(
            &self.repository,
            &["commit", "--quiet", "-m", "higher release source"],
        )?;
        self.source_sha = String::from_utf8(git(&self.repository, &["rev-parse", "HEAD"])?.stdout)?
            .trim()
            .to_owned();
        self.tag = format!("{PACKAGE}-v{VERSION}");
        git(&self.repository, &["tag", &self.tag])?;
        Ok(())
    }

    fn seed_matching_manual_draft(&self) -> Result<(), Box<dyn Error>> {
        fs::write(
            &self.state,
            format!(
                r#"{{"id":987,"tag_name":"{tag}","name":"{tag}","target_commitish":"{source_sha}","draft":true,"prerelease":false,"body":"Automated binary release for {tag}.","assets":[]}}"#,
                tag = self.tag,
                source_sha = self.source_sha,
            ),
        )?;
        Ok(())
    }
}

#[test]
fn publisher_rejects_existing_drafts_even_when_assets_are_empty_or_foreign()
-> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let fixture = ReleaseFixture::new(&scratch.0)?;
    let commands = mock_commands();
    let prep_script = scripts::prepare_assets(PACKAGE, BINARY);
    prepare_archives(&scratch.0, "1.0.0")?;
    let prepared = run_prepare(&prep_script, &scratch.0, "1.0.0")?;
    assert!(prepared.status.success(), "{}", stderr(&prepared));

    let publish_script = scripts::publish(PACKAGE, BINARY, "gh");
    // The mock leaves a partial draft before failing, modeling interruption or failed cleanup.
    let first = fixture.publish(&publish_script)?;
    assert!(
        !first.status.success(),
        "interrupted create unexpectedly succeeded"
    );
    assert!(stderr(&first).contains("leftover draft will be left untouched"));
    assert!(fixture.state.is_file(), "mock create left no draft behind");
    let verify_script = scripts::verify_source(&config(), BINARY, &commands);
    fixture.verify_source_with_hidden_draft(&verify_script)?;

    let duplicate = fixture.publish_with_duplicate_draft_pages(&publish_script, true)?;
    assert!(!duplicate.status.success());
    assert!(stderr(&duplicate).contains("multiple releases use the selected tag"));
    let calls = fs::read_to_string(&fixture.log)?;
    assert!(
        calls.contains("release-list-write:duplicate-draft-later-pages"),
        "{calls}"
    );
    assert_eq!(calls.matches("draft-detail:").count(), 0, "{calls}");
    assert_eq!(calls.matches("upload:").count(), 0, "{calls}");

    // A manual draft can match the generated tag/title/body while having no
    // assets. It must not be treated as an interrupted workflow-owned release.
    rewrite_json(&fixture.state, ".assets = []")?;
    assert_json(&fixture.state, ".draft == true and (.assets | length) == 0")?;
    fixture.reject_existing_draft(&publish_script)?;

    // The same rule applies when the matching draft contains foreign assets.
    rewrite_json(
        &fixture.state,
        ".assets = [{\"name\":\"notes.txt\",\"digest\":\"sha256:foreign\",\"size\":7}]",
    )?;
    assert_json(
        &fixture.state,
        ".draft == true and ([.assets[].name] == [\"notes.txt\"]) ",
    )?;
    fixture.reject_existing_draft(&publish_script)?;

    let calls = fs::read_to_string(fixture.log)?;
    assert_eq!(calls.matches("create-left-partial-draft").count(), 1);
    assert_eq!(
        calls.matches("release-list-read:hidden-draft").count(),
        1,
        "{calls}"
    );
    assert_eq!(
        calls.matches("release-list-write:draft-later-page").count(),
        2,
        "{calls}"
    );
    assert_eq!(
        calls
            .matches("release-list-write:duplicate-draft-later-pages")
            .count(),
        1,
        "{calls}"
    );
    assert_eq!(
        calls.matches(&format!("draft-detail:{RELEASE_ID}")).count(),
        0
    );
    assert_eq!(calls.matches("upload:").count(), 0);
    assert_eq!(calls.matches("patch").count(), 0);
    Ok(())
}

#[test]
fn hidden_higher_priority_draft_blocks_lower_eligible_tag_until_reconciled()
-> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let mut fixture = ReleaseFixture::new(&scratch.0)?;
    let lower_tag = fixture.tag.clone();
    fixture.add_higher_version_tag()?;
    let higher_tag = fixture.tag.clone();
    let higher_version = "2.0.0";

    let prep_script = scripts::prepare_assets(PACKAGE, BINARY);
    prepare_archives(&scratch.0, higher_version)?;
    let prepared = run_prepare(&prep_script, &scratch.0, higher_version)?;
    assert!(prepared.status.success(), "{}", stderr(&prepared));
    fixture.seed_matching_manual_draft()?;

    let commands = mock_commands();
    let verify_script = scripts::verify_source(&config(), BINARY, &commands);
    fixture.verify_source_with_hidden_draft(&verify_script)?;
    let selected = fs::read_to_string(&fixture.output)?;
    assert!(
        selected.contains(&format!("tag={higher_tag}\n")),
        "{selected}"
    );
    assert!(
        !selected.contains(&format!("tag={lower_tag}\n")),
        "{selected}"
    );

    let publish_script = scripts::publish(PACKAGE, BINARY, "gh");
    fixture.reject_existing_draft(&publish_script)?;
    let state = fs::read_to_string(&fixture.state)?;
    assert!(state.contains(&format!(r#""tag_name":"{higher_tag}""#)));
    assert!(state.contains(r#""assets":[]"#));

    // Once maintainers resolve the higher release as published, the read-only
    // resolver can see it and select the still-eligible lower tag.
    rewrite_json(&fixture.state, ".draft = false")?;
    fs::write(&fixture.output, "")?;
    let lower_selection = run_resolver(
        &verify_script,
        &fixture.repository,
        &fixture.root,
        &fixture.bin,
        &fixture.output,
        &fixture.source_sha,
        &fixture.state,
        &fixture.log,
    )?;
    assert!(
        lower_selection.status.success(),
        "{}",
        stderr(&lower_selection)
    );
    let selected = fs::read_to_string(&fixture.output)?;
    assert!(
        selected.contains(&format!("tag={lower_tag}\n")),
        "{selected}"
    );
    assert!(
        !selected.contains(&format!("tag={higher_tag}\n")),
        "{selected}"
    );
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

fn prepare_archives(root: &Path, version: &str) -> Result<(), Box<dyn Error>> {
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
        metadata: r#"python3 -c 'import json; line=[x for x in open("Cargo.toml") if x.startswith("version=")][0]; version=line.split(chr(61))[1].strip().strip(chr(39)); print(json.dumps({"packages":[{"name":"demo-package","version":version,"targets":[{"name":"demo-binary","kind":["bin"]}]}]}))'"#.to_owned(),
        rustc_version: String::new(),
        rust_version: String::new(),
        build_linux: String::new(),
        build_macos: String::new(),
        install_gh: String::new(),
        gh_prefix: "gh".to_owned(),
    }
}

fn run_prepare(script: &str, root: &Path, version: &str) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new("bash")
        .args(["-c", script])
        .current_dir(root)
        .env("SOURCE_SHA", "0123456789012345678901234567890123456789")
        .env("RELEASE_TAG", format!("{PACKAGE}-v{version}"))
        .env("RELEASE_VERSION", version)
        .env_remove("GH_TOKEN")
        .output()?)
}

fn run_publish(
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
