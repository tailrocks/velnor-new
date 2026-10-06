//! Closed Maven Central provenance for the Gradle native artifact payload.
//!
//! Gradle's artifact directory is shared by every repository lookup.  The
//! proof below stages only the fixed compile closure and records the
//! exact JAR fingerprints consumed by the output-cache policy.  Coordinates
//! are typed values; URLs never cross this boundary.

use std::collections::BTreeMap;

const PROOF: &str = include_str!("workloads_cache_gradle_artifacts.py");
const SANITIZE: &str = include_str!("workloads_cache_gradle_artifacts_sanitize.py");

/// The only Gradle homes that the artifact helper can address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GradleArtifactHome {
    Consumer,
    Producer,
}

impl GradleArtifactHome {
    const fn role(self) -> &'static str {
        match self {
            Self::Consumer => "consumer",
            Self::Producer => "producer",
        }
    }

    const fn path(self) -> &'static str {
        match self {
            Self::Consumer => "${{ runner.temp }}/velnor/native/gradle",
            Self::Producer => "${{ runner.temp }}/velnor/native/gradle-producer/gradle-home",
        }
    }

    const fn proof_id(self) -> &'static str {
        match self {
            Self::Consumer => "velnor-gradle-public-proof-consumer",
            Self::Producer => "velnor-gradle-public-proof-producer",
        }
    }

    const fn sanitizer_id(self) -> &'static str {
        match self {
            Self::Consumer => "velnor-gradle-artifacts-sanitize-consumer",
            Self::Producer => "velnor-gradle-artifacts-sanitize-producer",
        }
    }
}

/// One immutable public Maven coordinate and its Central checksum evidence.
#[derive(Debug, Clone, Copy, Eq, PartialEq, serde::Serialize)]
pub(crate) struct GradleArtifactDescriptor {
    pub(crate) group: &'static str,
    pub(crate) module: &'static str,
    pub(crate) version: &'static str,
    pub(crate) jar_algorithm: &'static str,
    pub(crate) jar_checksum: &'static str,
    pub(crate) jar_sha256: &'static str,
    pub(crate) pom_algorithm: &'static str,
    pub(crate) pom_checksum: &'static str,
}

const ARTIFACTS: [GradleArtifactDescriptor; 2] = [
    GradleArtifactDescriptor {
        group: "io.micronaut",
        module: "micronaut-core",
        version: "4.10.14",
        jar_algorithm: "sha256",
        jar_checksum: "2485a578736b3d013aecf17d5c1a4ee2669754af185d5bc6b5b12c7d685129ad",
        jar_sha256: "2485a578736b3d013aecf17d5c1a4ee2669754af185d5bc6b5b12c7d685129ad",
        pom_algorithm: "sha256",
        pom_checksum: "543125333530726e051998c583a748d2bfe3642f5253b9163f1b8f6f3b7abb99",
    },
    GradleArtifactDescriptor {
        group: "org.slf4j",
        module: "slf4j-api",
        version: "2.0.17",
        jar_algorithm: "sha1",
        jar_checksum: "d9e58ac9c7779ba3bf8142aff6c830617a7fe60f",
        jar_sha256: "7b751d952061954d5abfed7181c1f645d336091b679891591d63329c622eb832",
        pom_algorithm: "sha1",
        pom_checksum: "0570964f8e6716b09c354c6e334ba1a092464d85",
    },
];

/// The closed compile closure proven from the reviewed consumer POM.
pub(crate) const fn artifact_descriptors() -> &'static [GradleArtifactDescriptor] {
    &ARTIFACTS
}

/// Prove anonymous Maven Central bytes and stage their cache entries.
pub(crate) fn proof_step(
    home: GradleArtifactHome,
) -> Result<velnor_actions_contract::Step, crate::OrchestratorError> {
    let mut env = isolated_env(home);
    env.insert(
        "VELNOR_GRADLE_ARTIFACT_CANDIDATES".to_owned(),
        serde_json::to_string(artifact_descriptors()).map_err(|error| {
            crate::OrchestratorError::Contract {
                problem: error.to_string(),
            }
        })?,
    );
    let mut step = velnor_actions_workflow_renderer::ambient_shell_step(
        "Prove Gradle anonymous public artifacts",
        vec!["/bin/sh".to_owned(), "-c".to_owned(), proof_wrapper(home)?],
        env,
    )?;
    step.id = Some(
        velnor_actions_contract::StepId::new(home.proof_id()).map_err(|error| {
            crate::OrchestratorError::Contract {
                problem: error.to_string(),
            }
        })?,
    );
    Ok(step)
}

/// Remove every files-2.1 entry outside the proven JAR manifest.
pub(crate) fn sanitizer_step(
    home: GradleArtifactHome,
) -> Result<velnor_actions_contract::Step, crate::OrchestratorError> {
    let mut env = isolated_env(home);
    env.insert(
        "VELNOR_GRADLE_PUBLIC_PROOF_SAFE".to_owned(),
        format!("${{{{steps.{}.outputs.proof-safe}}}}", home.proof_id()),
    );
    let mut step = velnor_actions_workflow_renderer::ambient_shell_step(
        "Sanitize Gradle public artifact store",
        vec![
            "/bin/sh".to_owned(),
            "-c".to_owned(),
            sanitizer_wrapper(home)?,
        ],
        env,
    )?;
    step.id = Some(
        velnor_actions_contract::StepId::new(home.sanitizer_id()).map_err(|error| {
            crate::OrchestratorError::Contract {
                problem: error.to_string(),
            }
        })?,
    );
    Ok(step)
}

fn isolated_env(home: GradleArtifactHome) -> BTreeMap<String, String> {
    let mut env = BTreeMap::from([
        ("GRADLE_USER_HOME".to_owned(), home.path().to_owned()),
        ("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned()),
        (
            "VELNOR_GRADLE_ARTIFACT_HOME_ROLE".to_owned(),
            home.role().to_owned(),
        ),
        ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
    ]);
    for name in [
        "BASH_ENV",
        "ENV",
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "LD_AUDIT",
        "DYLD_INSERT_LIBRARIES",
        "DYLD_LIBRARY_PATH",
        "DYLD_FRAMEWORK_PATH",
        "DYLD_FALLBACK_FRAMEWORK_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "PYTHONPATH",
        "PYTHONHOME",
        "JAVA_TOOL_OPTIONS",
        "JDK_JAVA_OPTIONS",
        "_JAVA_OPTIONS",
        "GRADLE_OPTS",
    ] {
        env.insert(name.to_owned(), String::new());
    }
    env
}

fn proof_wrapper(
    home: GradleArtifactHome,
) -> Result<String, velnor_actions_workflow_renderer::RenderError> {
    let role = home.role();
    let command = velnor_actions_workflow_renderer::join_argv_for_run(&[
        "/usr/bin/python3".to_owned(),
        "-I".to_owned(),
        "-c".to_owned(),
        PROOF.to_owned(),
    ])?;
    Ok(format!(
        "set +e\n/usr/bin/env -i RUNNER_TEMP=\"$RUNNER_TEMP\" GRADLE_USER_HOME=\"$GRADLE_USER_HOME\" VELNOR_GRADLE_ARTIFACT_HOME_ROLE={role} VELNOR_GRADLE_ARTIFACT_CANDIDATES=\"$VELNOR_GRADLE_ARTIFACT_CANDIDATES\" GITHUB_OUTPUT=\"$GITHUB_OUTPUT\" {command}\ncode=$?\n\
         if test \"$code\" -ne 0; then\n\
         printf 'proof-safe=false\\nmanifest-path=\\n' >> \"$GITHUB_OUTPUT\"\n\
         fi\nexit 0"
    ))
}

fn sanitizer_wrapper(
    home: GradleArtifactHome,
) -> Result<String, velnor_actions_workflow_renderer::RenderError> {
    let role = home.role();
    let command = velnor_actions_workflow_renderer::join_argv_for_run(&[
        "/usr/bin/python3".to_owned(),
        "-I".to_owned(),
        "-c".to_owned(),
        SANITIZE.to_owned(),
    ])?;
    Ok(format!(
        "set +e\n/usr/bin/env -i RUNNER_TEMP=\"$RUNNER_TEMP\" GRADLE_USER_HOME=\"$GRADLE_USER_HOME\" VELNOR_GRADLE_ARTIFACT_HOME_ROLE={role} VELNOR_GRADLE_PUBLIC_PROOF_SAFE=\"$VELNOR_GRADLE_PUBLIC_PROOF_SAFE\" {command}\ncode=$?\n\
         safe=false\nif test \"$code\" -eq 0; then safe=true; fi\n\
         printf 'cache-safe=%s\\n' \"$safe\" >> \"$GITHUB_OUTPUT\"\n\
         exit 0"
    ))
}

#[cfg(test)]
#[path = "workloads_cache_gradle_artifacts_tests.rs"]
mod tests;
