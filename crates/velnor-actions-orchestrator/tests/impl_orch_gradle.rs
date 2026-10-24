//! Gradle stack generation: detection, fail-closed findings and the job shape.

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use crate::impl_common::{TestResult, make_repo};

const CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.gradle]\nroots = [\"backend\"]\n[stacks.gradle.projects.\":core\"]\nservices = [\"postgres\"]\n";

const JAR: &[u8] =
    include_bytes!("../../velnor-actions-gradle/tests/fixtures/gradle-wrapper-9.5.1.jar");
const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn put(root: &Path, rel: &str, text: &str) -> TestResult {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().ok_or("parent")?)?;
    fs::write(path, text)?;
    Ok(())
}

/// A compliant two-project Gradle root in `backend/`; `core` reads the database.
pub(crate) fn gradle_root(repo: &Path) -> TestResult {
    put(
        repo,
        "backend/settings.gradle.kts",
        "rootProject.name = \"m\"\ninclude(\"core\", \"app\")\n",
    )?;
    put(repo, "backend/gradlew", "#!/bin/sh\n")?;
    put(
        repo,
        "backend/gradle/wrapper/gradle-wrapper.properties",
        "distributionSha256Sum=bafc141b619ad6350fd975fc903156dd5c151998cc8b058e8c1044ab5f7b031f\n\
         distributionUrl=https\\://services.gradle.org/distributions/gradle-9.5.1-bin.zip\n",
    )?;
    let jar = repo.join("backend/gradle/wrapper/gradle-wrapper.jar");
    fs::write(jar, JAR)?;
    put(
        repo,
        "backend/core/build.gradle.kts",
        "dependencies { api(libs.jooq) }\nval h = System.getenv(\"POSTGRESQL_DB_HOST\")\n",
    )?;
    put(repo, "backend/core/gradle.lockfile", "# lock\n")?;
    put(
        repo,
        "backend/app/build.gradle.kts",
        "dependencies { api(projects.core) }\n",
    )?;
    put(repo, "backend/app/gradle.lockfile", "# lock\n")?;
    put(
        repo,
        "backend/gradle/verification-metadata.xml",
        &format!(
            "<verification-metadata><configuration><verify-metadata>true</verify-metadata></configuration>\
             <components><component group=\"g\" name=\"n\" version=\"1\"><artifact name=\"n-1.jar\">\
             <sha256 value=\"{SHA}\"/></artifact></component></components></verification-metadata>"
        ),
    )
}

fn render(repo: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let prep = prepare(repo)?;
    let out = TempDir::new()?;
    let preview = out.path().join("preview");
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    Ok(fs::read_to_string(
        preview.join(".github/workflows/ci.yml"),
    )?)
}

#[test]
fn a_compliant_root_gets_one_job_per_project_with_a_database_service_where_configured() -> TestResult
{
    let repo = make_repo(CONFIG)?;
    gradle_root(repo.path())?;
    let yaml = render(repo.path())?;
    let section = |id: &str| -> Result<String, Box<dyn std::error::Error>> {
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
    };
    let core = section("gradle-backend-core")?;
    assert!(core.contains("name: Gradle / core"), "{core}");
    assert!(
        core.contains("services:") && core.contains("postgres@sha256:"),
        "{core}"
    );
    assert!(core.contains("VELNOR_GRADLE_DATABASE: \"1\""), "{core}");
    let app = section("gradle-backend-app")?;
    assert!(!app.contains("services:"), "{app}");
    for job in [&core, &app] {
        assert!(job.contains("VELNOR_GRADLE_TASKS"), "{job}");
        assert!(job.contains(":assemble") && job.contains(":check"), "{job}");
        assert!(job.contains("VELNOR_TOOL_JAVA"), "{job}");
        let setup = job.find("Setup Mise").ok_or("mise")?;
        let run = job.find("Gradle assemble and check").ok_or("run")?;
        assert!(setup < run, "mise before the run: {job}");
    }
    assert!(yaml.contains("Resolve project tool versions"), "{yaml}");
    Ok(())
}

#[test]
fn a_root_that_violates_the_contract_fails_generation_with_finding_codes() -> TestResult {
    let repo = make_repo(CONFIG)?;
    gradle_root(repo.path())?;
    fs::remove_file(repo.path().join("backend/app/gradle.lockfile"))?;
    fs::remove_file(repo.path().join("backend/gradle/verification-metadata.xml"))?;
    let mut tampered = JAR.to_vec();
    tampered[300] ^= 1;
    fs::write(
        repo.path()
            .join("backend/gradle/wrapper/gradle-wrapper.jar"),
        tampered,
    )?;
    let err = match prepare(repo.path()) {
        Err(err) => err.to_string(),
        Ok(prep) => {
            let out = TempDir::new()?;
            generate(
                &prep,
                &GenerateOptions {
                    output_dir: Some(out.path().join("p")),
                },
            )
            .expect_err("findings must fail generation")
            .to_string()
        }
    };
    for code in [
        "backend: gradle_wrapper_jar_unverified",
        "gradle_dependency_lock_missing::app",
        "gradle_verification_metadata_missing",
    ] {
        assert!(err.contains(code), "{code} in {err}");
    }
    Ok(())
}

pub(crate) fn scale_set_config(config: &str) -> String {
    let config = config
        .replace("schema = 1", "schema = 2")
        .replace("name = \"CI\"", "name = \"CI\"\npolicy = \"consumer-v1\"");
    format!(
        "{config}\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nmode = \"scale-set\"\n\
[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\
[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"java-integration-scale-set\"\nlabels = [\"velnor\", \"java-integration-scale-set\"]\nplatform = \"linux/amd64\"\njob_container_image = \"ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n"
    )
}

pub(crate) fn audit_scale_set_jobs(path: &str, yaml: &str) -> TestResult {
    use velnor_actions_contract::config::ValidatedImageReference;
    use velnor_actions_contract::{ExecutionMode, ScaleSetSelector};
    use velnor_actions_workflow_renderer::{RenderedFile, WorkflowRoutingPolicy};
    let policy = WorkflowRoutingPolicy::configured(
        ExecutionMode::ScaleSet,
        ScaleSetSelector::try_new(
            "java-integration-scale-set",
            &["velnor".to_owned(), "java-integration-scale-set".to_owned()],
        )?,
        Some(ValidatedImageReference::parse(
            "ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )?),
    );
    velnor_actions_workflow_renderer::workflow_routing::validate_generated_workflow_calls(
        &[RenderedFile {
            path: path.to_owned(),
            bytes: yaml.to_owned(),
        }],
        &policy,
    )?;
    Ok(())
}

#[test]
fn scale_set_mode_routes_gradle_and_control_jobs() -> TestResult {
    let repo = make_repo(&scale_set_config(CONFIG))?;
    gradle_root(repo.path())?;
    let yaml = render(repo.path())?;
    assert_eq!(yaml, render(repo.path())?);
    audit_scale_set_jobs(".github/workflows/ci.yml", &yaml)?;
    for id in [
        "plan",
        "required",
        "gradle-backend-core",
        "gradle-backend-app",
    ] {
        assert!(yaml.contains(&format!("\n  {id}:\n")), "{yaml}");
    }
    let facts = velnor_actions_workflow_renderer::extract_workflow_authority(&[
        velnor_actions_workflow_renderer::RenderedFile {
            path: ".github/workflows/ci.yml".to_owned(),
            bytes: yaml.clone(),
        },
    ])?;
    let jobs = &facts[0].jobs;
    let required = jobs
        .iter()
        .find(|job| job.job_key == "required")
        .ok_or("Required job is absent.")?;
    for id in ["gradle-backend-core", "gradle-backend-app"] {
        assert!(required.needs.iter().any(|need| need == id), "{required:?}");
    }
    assert_eq!(
        jobs.iter()
            .filter(|job| job.job_key.starts_with("gradle-"))
            .count(),
        2
    );
    assert!(
        yaml.contains("postgres@sha256:") && yaml.contains("VELNOR_GRADLE_TASKS"),
        "{yaml}"
    );
    assert!(yaml.contains("VELNOR_TOOL_JAVA"), "{yaml}");
    Ok(())
}
