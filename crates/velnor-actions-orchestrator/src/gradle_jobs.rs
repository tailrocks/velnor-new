//! Generated Gradle jobs: one per project (hard decision 5).
//!
//! Detection is static (`velnor-actions-gradle`): a Gradle root is a
//! directory with `settings.gradle(.kts)`, `gradlew` and the wrapper
//! properties. Generation fails closed with the adapter's finding codes when
//! a root violates the stack contract (wrapper gates, dynamic settings,
//! dependency locking, verification metadata). Each project with a build
//! script gets a job that verifies the wrapper before `./gradlew` ever runs,
//! retrieves the lock's Maven bundles through the private-source block, and
//! runs `assemble` and `check` as one `--continue` invocation.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_actions_contract::{
    Job, JobTimeout, Permissions, ServiceContainer, Step, VelnorConfig, slugify_segment,
};
use velnor_actions_gradle::analysis::analyze_root;
use velnor_actions_gradle::script::{
    INIT_SCRIPT_NAME, user_home_properties, verify_wrapper_script,
};
use velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
use velnor_actions_workflow_renderer::{ambient_shell_step, mise_setup_step};

use crate::OrchestratorError;
use crate::config::private_wiring::PrivateWiring;
use crate::safe_read::{RepoBytes, read_repo_bytes};

/// Largest Gradle file read (verification metadata can be megabytes).
const MAX_GRADLE_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Job id prefix of generated Gradle jobs.
pub(crate) const GRADLE_JOB_PREFIX: &str = "gradle-";

/// One analysed Gradle root and the facts the job builder needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GradleRoot {
    /// Repository-relative root directory (`""` for the repository root).
    pub(crate) root: String,
    /// Projects with a build script: `(path, needs a database)`.
    pub(crate) projects: Vec<(String, bool)>,
}

fn contract(problem: String) -> OrchestratorError {
    OrchestratorError::Contract { problem }
}

fn join(root: &str, rel: &str) -> String {
    if root.is_empty() {
        rel.to_owned()
    } else {
        format!("{root}/{rel}")
    }
}

/// Directories holding a settings script, `gradlew` and the wrapper properties.
fn root_dirs(files: &[String]) -> Vec<String> {
    let set: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let mut roots: Vec<String> = files
        .iter()
        .filter_map(|file| {
            let dir = file
                .strip_suffix("/settings.gradle.kts")
                .or_else(|| file.strip_suffix("/settings.gradle"))
                .or_else(|| {
                    (file == "settings.gradle.kts" || file == "settings.gradle").then_some("")
                })?;
            let has = |rel: &str| set.contains(join(dir, rel).as_str());
            (has("gradlew") && has("gradle/wrapper/gradle-wrapper.properties"))
                .then(|| dir.to_owned())
        })
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// Detect and analyse every Gradle root; fail closed on any finding.
///
/// # Errors
/// A contract error listing `<root>: <finding>` codes.
pub(crate) fn discover(
    repo: &Path,
    files: &[String],
) -> Result<Vec<GradleRoot>, OrchestratorError> {
    let mut found = Vec::new();
    let mut problems = Vec::new();
    for root in root_dirs(files) {
        let prefix = if root.is_empty() {
            String::new()
        } else {
            format!("{root}/")
        };
        let rel: BTreeSet<String> = files
            .iter()
            .filter_map(|file| file.strip_prefix(&prefix).map(str::to_owned))
            .collect();
        let read = |path: &str| -> Option<Vec<u8>> {
            match read_repo_bytes(repo, &join(&root, path), MAX_GRADLE_FILE_BYTES) {
                Ok(RepoBytes::Bytes(bytes)) => Some(bytes),
                _ => None,
            }
        };
        let analysis = analyze_root(&root, &rel, &read);
        problems.extend(
            analysis
                .findings
                .iter()
                .map(|code| format!("{root}: {code}")),
        );
        let Some(model) = &analysis.settings else {
            continue;
        };
        found.push(GradleRoot {
            root: root.clone(),
            projects: projects_with_scripts(&model.projects, &analysis.edges, &rel, &read),
        });
    }
    if problems.is_empty() {
        Ok(found)
    } else {
        Err(contract(format!(
            "gradle_stack_findings:{}",
            problems.join("; ")
        )))
    }
}

/// Projects with a build script, flagged when their closure reads the database.
fn projects_with_scripts(
    projects: &[velnor_actions_gradle::settings::Project],
    edges: &BTreeMap<String, BTreeSet<String>>,
    files: &BTreeSet<String>,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Vec<(String, bool)> {
    let dir_of: BTreeMap<&str, &str> = projects
        .iter()
        .map(|project| (project.path.as_str(), project.dir.as_str()))
        .collect();
    let uses_database = |path: &str| -> bool {
        let dir = dir_of.get(path).copied().unwrap_or_default();
        ["build.gradle.kts", "build.gradle"].iter().any(|name| {
            let full = format!("{dir}/{name}");
            files.contains(&full)
                && read(&full)
                    .is_some_and(|bytes| String::from_utf8_lossy(&bytes).contains("POSTGRESQL_DB_"))
        })
    };
    projects
        .iter()
        .filter(|project| {
            ["build.gradle.kts", "build.gradle"]
                .iter()
                .any(|name| files.contains(&format!("{}/{name}", project.dir)))
        })
        .map(|project| {
            let mut closure = BTreeSet::from([project.path.clone()]);
            let mut queue = vec![project.path.clone()];
            while let Some(next) = queue.pop() {
                for dep in edges.get(&next).into_iter().flatten() {
                    if closure.insert(dep.clone()) {
                        queue.push(dep.clone());
                    }
                }
            }
            let database = closure.iter().any(|path| uses_database(path));
            (project.path.clone(), database)
        })
        .collect()
}

/// Job id for one project.
fn job_id(root: &str, project: &str) -> String {
    let scope = if root.is_empty() {
        String::new()
    } else {
        format!("{}-", slugify_segment(root))
    };
    format!("{GRADLE_JOB_PREFIX}{scope}{}", slugify_segment(project))
}

/// The Gradle command script of one project job.
fn run_script(root: &str, project: &str, max_workers: u8) -> String {
    let props = user_home_properties();
    let init = velnor_actions_gradle::script::init_script();
    let dir = if root.is_empty() { "." } else { root };
    format!(
        r##"set -eu
gradle_home="$RUNNER_TEMP/velnor/gradle"
mkdir -p "$gradle_home/init.d"
cat > "$gradle_home/gradle.properties" <<'VELNOR_PROPS'
{props}VELNOR_PROPS
cat > "$gradle_home/init.d/{INIT_SCRIPT_NAME}" <<'VELNOR_INIT'
{init}VELNOR_INIT
cd {dir}
export GRADLE_USER_HOME="$gradle_home"
export VELNOR_MAVEN_ROOT="$RUNNER_TEMP/velnor/deps/maven"
./gradlew --console=plain --no-daemon --continue --dependency-verification strict --max-workers={max_workers} {project}:assemble {project}:check
"##
    )
}

const INSTALL_JDK: &str = r##"set -eu
mise --no-config --no-env --no-hooks install "java@${VELNOR_TOOL_JAVA}"
java_home=$(mise --no-config --no-env --no-hooks where "java@${VELNOR_TOOL_JAVA}")
[ -x "$java_home/bin/java" ] || { printf 'velnor: JDK %s has no bin/java\n' "$VELNOR_TOOL_JAVA" >&2; exit 1; }
printf 'JAVA_HOME=%s\n' "$java_home" >> "$GITHUB_ENV"
"##;

fn shell(
    name: &str,
    script: String,
    env: BTreeMap<String, String>,
) -> Result<Step, OrchestratorError> {
    ambient_shell_step(name, vec!["sh".to_owned(), "-c".to_owned(), script], env)
        .map_err(OrchestratorError::from)
}

/// Build every Gradle job.
///
/// # Errors
/// Pin, step-rendering and private-source errors.
pub(crate) fn build_jobs(
    config: &VelnorConfig,
    roots: &[GradleRoot],
    acquire: Option<&Step>,
    private: Option<&PrivateWiring>,
) -> Result<Vec<(String, Job)>, OrchestratorError> {
    let mut jobs = Vec::new();
    let setup = crate::pins::resolve_mise_setup_for_release_target(
        config,
        velnor_actions_contract::ReleaseTarget::LinuxX86_64,
    )?;
    let minutes = config.workflow.crate_timeout_minutes;
    for root in roots {
        let mut prefix = Vec::new();
        if let Some(acquire) = acquire {
            prefix.push(acquire.clone());
        }
        if let Some(private) = private {
            prefix.extend(private.steps_for_gradle(std::slice::from_ref(&root.root))?);
        }
        for (project, database) in &root.projects {
            let mut steps = vec![crate::workflow::wire_w1::checkout_step()?];
            steps.extend(prefix.iter().cloned());
            steps.push(mise_setup_step(&setup).map_err(OrchestratorError::from)?);
            steps.push(shell(
                "Install JDK",
                INSTALL_JDK.to_owned(),
                BTreeMap::new(),
            )?);
            steps.push(shell(
                "Verify Gradle wrapper",
                verify_wrapper_script(&root.root),
                BTreeMap::new(),
            )?);
            let run = shell(
                "Gradle assemble and check",
                run_script(&root.root, project, 4),
                if *database {
                    BTreeMap::from([
                        ("POSTGRESQL_DB_HOST".to_owned(), "127.0.0.1".to_owned()),
                        ("POSTGRESQL_DB_PORT".to_owned(), "40000".to_owned()),
                    ])
                } else {
                    BTreeMap::new()
                },
            )?;
            steps.push(run);
            let name = project.trim_start_matches(':');
            jobs.push((
                job_id(&root.root, name),
                Job {
                    services: if *database {
                        vec![
                            ServiceContainer::postgres(18)
                                .map_err(|err| contract(err.to_string()))?,
                        ]
                    } else {
                        Vec::new()
                    },
                    check_runner: None,
                    display_name: format!("Gradle / {name}"),
                    runs_on: config.workflow.runner_label.clone().unwrap_or_else(|| {
                        velnor_actions_contract::config::LATEST_RUNNER_LABEL.to_owned()
                    }),
                    timeout_minutes: JobTimeout::new(minutes)?,
                    needs: vec![PLAN_JOB_ID.to_owned()],
                    condition: None,
                    permissions: Some(Permissions::default()),
                    environment: None,
                    steps,
                    cache_mode: None,
                },
            ));
        }
    }
    jobs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(jobs)
}

#[cfg(test)]
#[path = "gradle_jobs_tests.rs"]
mod tests;
