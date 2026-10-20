//! Gradle stack generation: detection, fail-closed findings and the job shape.

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use crate::impl_common::{TestResult, config_with_branch, make_repo};

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
fn gradle_root(repo: &Path) -> TestResult {
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
fn a_compliant_root_gets_one_job_per_project_with_a_database_service_where_needed() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    gradle_root(repo.path())?;
    let yaml = render(repo.path())?;
    let section = |id: &str| -> String {
        let start = yaml
            .find(&format!("\n  {id}:\n"))
            .unwrap_or_else(|| panic!("{id} job in {yaml}"));
        let rest = &yaml[start + 1..];
        rest.split("\n  ")
            .skip(1)
            .next()
            .map_or(rest.to_owned(), |_| {
                rest.split("\n\n").next().unwrap_or(rest).to_owned()
            })
    };
    let core = section("gradle-backend-core");
    assert!(core.contains("name: Gradle / core"), "{core}");
    assert!(
        core.contains("services:") && core.contains("postgres@sha256:"),
        "{core}"
    );
    assert!(core.contains("POSTGRESQL_DB_PORT"), "{core}");
    let app = section("gradle-backend-app");
    assert!(!app.contains("services:"), "{app}");
    for job in [&core, &app] {
        let wrapper = job.find("Verify Gradle wrapper").ok_or("wrapper step")?;
        let gradle = job
            .find("--dependency-verification strict")
            .ok_or("gradle step")?;
        let jdk = job.find("Install JDK").ok_or("jdk step")?;
        assert!(jdk < wrapper && wrapper < gradle, "order: {job}");
        assert!(
            job.contains("--continue") && job.contains("assemble") && job.contains(":check"),
            "{job}"
        );
        assert!(job.contains("VELNOR_TOOL_JAVA"), "{job}");
    }
    assert!(yaml.contains("Resolve project tool versions"), "{yaml}");
    Ok(())
}

#[test]
fn a_root_that_violates_the_contract_fails_generation_with_finding_codes() -> TestResult {
    let repo = make_repo(config_with_branch())?;
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
