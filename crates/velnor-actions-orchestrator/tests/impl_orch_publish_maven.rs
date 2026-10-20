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

#[test]
fn scale_set_mode_keeps_maven_publication_authority() -> TestResult {
    let config = crate::impl_orch_gradle::scale_set_config(CONFIG);
    let repo = repo(&config, PROPERTIES)?;
    let yaml = preview(repo.path())?;
    assert_eq!(yaml, preview(repo.path())?);
    crate::impl_orch_gradle::audit_scale_set_jobs(PUBLISH_PATH, &yaml)?;
    let hosted_repo = self::repo(CONFIG, PROPERTIES)?;
    let hosted_yaml = preview(hosted_repo.path())?;
    let hosted_header = hosted_yaml
        .split_once("jobs:\n")
        .ok_or("Hosted jobs are absent.")?
        .0;
    let scale_set_header = yaml
        .split_once("jobs:\n")
        .ok_or("Scale Set jobs are absent.")?
        .0;
    assert_eq!(hosted_header, scale_set_header);
    let tag_glob = velnor_actions_contract::config::RELEASE_TAG_GLOB;
    assert_eq!(scale_set_header.matches(tag_glob).count(), 1);
    assert_eq!(hosted_header.matches(tag_glob).count(), 1);
    let facts = |bytes: String| {
        velnor_actions_workflow_renderer::extract_workflow_authority(&[
            velnor_actions_workflow_renderer::RenderedFile {
                path: PUBLISH_PATH.to_owned(),
                bytes: bytes.replacen(tag_glob, "processor-shared-libs-v2.3.0", 1),
            },
        ])
    };
    assert_eq!(facts(hosted_yaml)?, facts(yaml.clone())?);
    let authority_facts = facts(yaml.clone())?;
    assert_eq!(authority_facts[0].jobs.len(), 4);
    let authority = section(&yaml, "publish-authority")?;
    assert!(!authority.contains("checkout"), "{authority}");
    let build = section(&yaml, "maven-build-processor-shared-libs")?;
    assert!(
        build.contains("- publish-authority")
            && build
                .contains("needs.publish-authority.outputs.artifact == 'processor-shared-libs'"),
        "{build}"
    );
    assert!(
        build.contains("contents: read") && !build.contains("environment:"),
        "{build}"
    );
    let attest = section(&yaml, "maven-attest-processor-shared-libs")?;
    assert!(
        attest.contains("- maven-build-processor-shared-libs")
            && attest.contains("id-token: write")
            && attest.contains("attestations: write"),
        "{attest}"
    );
    let publish = section(&yaml, "maven-publish-processor-shared-libs")?;
    assert!(
        publish.contains("- maven-attest-processor-shared-libs")
            && publish.contains("environment: release")
            && publish.contains("contents: write"),
        "{publish}"
    );
    for job in [&attest, &publish] {
        assert!(
            !job.contains("checkout") && !job.contains("VELNOR_INTERNAL_OP"),
            "{job}"
        );
    }
    assert!(!publish.contains("--clobber"), "{publish}");
    assert!(yaml.contains("cancel-in-progress: false"), "{yaml}");
    Ok(())
}
