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
    assert!(build.contains("- publish-authority"), "{build}");
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
        attest.contains("- maven-build-processor-shared-libs"),
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
        publish.contains("- maven-attest-processor-shared-libs"),
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

const PRIVATE_CONFIG: &str = "[private_sources]\nowner = \"ChainArgos\"\napp_id_variable = \"VELNOR_DEPS_APP_ID\"\n\
app_private_key_secret = \"VELNOR_DEPS_APP_PRIVATE_KEY\"\n\
[[private_sources.maven_bundles]]\nid = \"libs\"\nrepository = \"ChainArgos/processor-shared-java\"\n\
tag = \"processor-shared-libs-v1.0.0\"\nasset = \"processor-shared-libs-1.0.0-maven.zip\"\n";

fn lock() -> String {
    format!(
        "schema = 1\n[[maven_bundles]]\nid = \"libs\"\nrepository = \"ChainArgos/processor-shared-java\"\n\
         tag = \"processor-shared-libs-v1.0.0\"\ncommit = \"{}\"\nasset = \"processor-shared-libs-1.0.0-maven.zip\"\n\
         sha256 = \"{}\"\n",
        "0123456789abcdef0123456789abcdef01234567",
        "89abcdef0123456789abcdef0123456789abcdef0123456789abcdef01234567"
    )
}

#[test]
fn private_maven_bundles_are_minted_fetched_and_revoked_around_the_build_only() -> TestResult {
    let config = format!("{CONFIG}{PRIVATE_CONFIG}");
    let repo = make_repo(&config)?;
    gradle_root(repo.path())?;
    fs::write(repo.path().join("backend/gradle.properties"), PROPERTIES)?;
    fs::write(repo.path().join(".velnor/deps.lock"), lock())?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
        repo.path(),
    )?;
    git(&["add", "."], repo.path())?;
    git(&["commit", "-m", "private maven fixture"], repo.path())?;
    let yaml = preview(repo.path())?;
    let build = section(&yaml, "maven-build-processor-shared-libs")?;
    let at = |needle: &str| build.find(needle).ok_or(format!("{needle} in {build}"));
    // The token is revoked before any repository build logic runs.
    assert!(
        at("create-github-app-token")? < at("private-fetch-v1")?,
        "{build}"
    );
    assert!(
        at("private-fetch-v1")? < at("private-revoke-v1")?,
        "{build}"
    );
    assert!(
        at("private-revoke-v1")? < at("Gradle mavenBundle")?,
        "{build}"
    );
    assert!(build.contains("steps.deps-token.outputs.token"), "{build}");
    assert!(build.contains("permission-contents: read"), "{build}");
    for job in [
        "maven-attest-processor-shared-libs",
        "maven-publish-processor-shared-libs",
    ] {
        let text = section(&yaml, job)?;
        assert!(
            !text.contains("create-github-app-token") && !text.contains("VELNOR_DEPS"),
            "{job}: no private-source token outside the build: {text}"
        );
    }
    Ok(())
}

const DB_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[stacks.gradle]\nroots = [\"backend\"]\ntimeout_minutes = 45\n\
[stacks.gradle.projects.\":core\"]\nservices = [\"postgres\"]\n\
[[publish.maven]]\nid = \"processor-shared-libs\"\nroot = \"backend\"\n";

#[test]
fn a_root_whose_ci_declares_postgres_gets_the_same_service_in_the_bundle_build() -> TestResult {
    let repo = repo(DB_CONFIG, PROPERTIES)?;
    let yaml = preview(repo.path())?;
    let build = section(&yaml, "maven-build-processor-shared-libs")?;
    assert!(build.contains("VELNOR_GRADLE_DATABASE: \"1\""), "{build}");
    assert!(
        build.contains("services:") && build.contains("postgres@sha256:"),
        "{build}"
    );
    assert!(build.contains("- 40000:5432"), "{build}");
    assert!(
        build.contains("permissions:\n      contents: read\n") && !build.contains("environment:"),
        "{build}"
    );
    let attest = section(&yaml, "maven-attest-processor-shared-libs")?;
    let publish = section(&yaml, "maven-publish-processor-shared-libs")?;
    assert!(
        !attest.contains("services:") && !publish.contains("services:"),
        "only the unprivileged build job carries the database"
    );
    assert!(attest.contains("attest"), "{attest}");
    Ok(())
}

#[test]
fn a_root_without_a_database_keeps_a_service_free_bundle_build() -> TestResult {
    let repo = repo(CONFIG, PROPERTIES)?;
    let yaml = preview(repo.path())?;
    let build = section(&yaml, "maven-build-processor-shared-libs")?;
    assert!(!build.contains("services:"), "{build}");
    assert!(build.contains("VELNOR_GRADLE_DATABASE: \"0\""), "{build}");
    Ok(())
}
