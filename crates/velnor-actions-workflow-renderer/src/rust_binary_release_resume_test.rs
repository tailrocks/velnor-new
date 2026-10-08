use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{SystemTime, UNIX_EPOCH};

use super::scripts;

#[path = "rust_binary_release_resume_mock.rs"]
mod mock;
#[path = "rust_binary_release_resume_support.rs"]
mod support;
use support::{
    assert_json, config, create_tagged_repository, git, install_mock_gh, mock_commands,
    prepare_archives, rewrite_json, run_prepare, run_publish, run_resolver, stderr,
};

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
