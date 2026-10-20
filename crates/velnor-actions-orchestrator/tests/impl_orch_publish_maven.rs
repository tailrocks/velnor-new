//! `[[publish.maven]]`: the tag-driven Maven bundle chain over a Gradle root.

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare, render_staged_tree};

use crate::impl_common::{TestResult, err_of, git, make_repo};
use crate::impl_orch_gradle::gradle_root;

const PUBLISH_PATH: &str = ".github/workflows/publish.yml";

const CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[stacks.gradle]\nroots = [\"backend\"]\ntimeout_minutes = 45\n\
[[publish.maven]]\nid = \"processor-shared-libs\"\nroot = \"backend\"\n";

fn repo(config: &str, properties: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    let root = repo.path();
    gradle_root(root)?;
    fs::write(root.join("backend/gradle.properties"), properties)?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
        root,
    )?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "maven fixture"], root)?;
    Ok(repo)
}

const PROPERTIES: &str = "version=2.3.0\nchainargos.bundleId=processor-shared-libs\n";

fn preview(repo: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let parent = TempDir::new()?;
    let out = parent.path().join("preview");
    generate(
        &prepare(repo)?,
        &GenerateOptions {
            output_dir: Some(out.clone()),
        },
    )?;
    Ok(fs::read_to_string(out.join(PUBLISH_PATH))?)
}

fn section(yaml: &str, id: &str) -> Result<String, Box<dyn std::error::Error>> {
    let start = yaml
        .find(&format!("\n  {id}:\n"))
        .ok_or(format!("{id} job in {yaml}"))?;
    let mut lines = yaml[start + 1..].lines();
    let mut out = lines.next().unwrap_or_default().to_owned();
    for line in lines {
        if line.starts_with("  ") && !line.starts_with("   ") {
            break;
        }
        out.push('\n');
        out.push_str(line);
    }
    Ok(out)
}

#[test]
fn a_maven_entry_emits_the_three_role_chain_behind_the_tag_authority() -> TestResult {
    let repo = repo(CONFIG, PROPERTIES)?;
    let yaml = preview(repo.path())?;
    assert!(yaml.contains("processor-shared-libs"), "{yaml}");
    assert!(
        yaml.contains("contents/backend/gradle.properties?ref=$GITHUB_SHA"),
        "version read at the tagged commit:\n{yaml}"
    );
    let build = section(&yaml, "maven-build-processor-shared-libs")?;
    assert!(build.contains("needs: publish-authority"), "{build}");
    assert!(
        build.contains("needs.publish-authority.outputs.artifact == 'processor-shared-libs'"),
        "{build}"
    );
    assert!(
        build.contains("permissions:\n      contents: read\n"),
        "{build}"
    );
    assert!(!build.contains("environment:"), "{build}");
    assert!(
        build.contains("VELNOR_INTERNAL_OP: gradle-run-v1"),
        "{build}"
    );
    assert!(
        build.contains("VELNOR_GRADLE_TASKS: \":mavenBundle\"")
            && build.contains("VELNOR_GRADLE_ROOT: backend")
            && build.contains("VELNOR_GRADLE_DATABASE: \"0\""),
        "{build}"
    );
    let at = |needle: &str| build.find(needle).ok_or(format!("{needle} in {build}"));
    assert!(
        at("actions/checkout")? < at("Acquire")? || at("Acquire").is_err(),
        "{build}"
    );
    assert!(at("Setup Mise")? < at("Gradle mavenBundle")?, "{build}");
    assert!(
        at("Gradle mavenBundle")? < at("Collect Maven bundle")?,
        "{build}"
    );
    assert!(
        at("Collect Maven bundle")? < at("Upload Maven bundle")?,
        "{build}"
    );
    assert!(build.contains("backend/build/maven-bundle"), "{build}");
    assert!(yaml.contains("Resolve project tool versions"), "{yaml}");

    let attest = section(&yaml, "maven-attest-processor-shared-libs")?;
    assert!(
        attest.contains("needs: maven-build-processor-shared-libs"),
        "{attest}"
    );
    assert!(
        attest.contains("id-token: write") && attest.contains("attestations: write"),
        "{attest}"
    );
    assert!(attest.contains("/bundle/*.zip"), "{attest}");
    assert!(
        !attest.contains("checkout") && !attest.contains("gradle"),
        "{attest}"
    );

    let publish = section(&yaml, "maven-publish-processor-shared-libs")?;
    assert!(
        publish.contains("needs: maven-attest-processor-shared-libs"),
        "{publish}"
    );
    assert!(publish.contains("environment: release"), "{publish}");
    assert!(publish.contains("contents: write"), "{publish}");
    assert!(
        publish.contains("processor-shared-libs-$ver-maven.zip.sha256")
            && !publish.contains("--clobber")
            && !publish.contains("checkout")
            && !publish.contains("VELNOR_INTERNAL_OP"),
        "{publish}"
    );
    Ok(())
}

#[test]
fn generation_is_deterministic() -> TestResult {
    let repo = repo(CONFIG, PROPERTIES)?;
    assert_eq!(preview(repo.path())?, preview(repo.path())?);
    Ok(())
}

fn rejection(config: &str, properties: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repo = repo(config, properties)?;
    Ok(err_of(
        render_staged_tree(&prepare(repo.path())?),
        "stale maven entry",
    )?
    .to_string())
}

#[test]
fn a_stale_or_inconsistent_declaration_fails_generation() -> TestResult {
    let text = rejection(CONFIG, "version=2.3\n")?;
    assert!(
        text.contains("version_not_literal:backend/gradle.properties"),
        "{text}"
    );
    let text = rejection(
        &CONFIG.replacen("root = \"backend\"", "root = \"other\"", 2),
        PROPERTIES,
    )?;
    assert!(text.contains("not_a_stacks_gradle_root"), "{text}");
    Ok(())
}
