//! This module defines container image build jobs that use no credentials.
//! Each job builds one image for one platform.
//! It checks out the repository and checks the Dockerfile against its saved digest.
//! It creates a Buildx builder from a pinned `BuildKit` image.
//! It builds an OCI layout with an SBOM and maximum provenance.
//! It removes the builder after every exit and uploads the layout.
//! Each job has only the `contents: read` permission.
//! The build receives no credentials, secret mounts, build arguments, or remote cache.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{
    ImageConfig, ImagePlatform, Job, JobTimeout, PermissionLevel, Permissions, Step,
};

use super::SCALE_TASK_NAME_SUFFIX;
use crate::{RenderError, shell_step, steps};

#[cfg(test)]
#[path = "image_build_jobs_tests.rs"]
mod tests;

/// Digest-pinned `BuildKit` image for the task-owned builder (moby/buildkit v0.34.0).
pub const BUILDKIT_IMAGE: &str =
    "moby/buildkit@sha256:b059f8d7226d0b326bc871af489a5dddf65e36af4d20d14251b072974d9ee87c";

/// This constant names the source check step.
pub(crate) const VERIFY_IMAGE_SOURCE_NAME: &str = "Verify image Dockerfile source";
/// Native Docker host step name.
pub(crate) const VERIFY_IMAGE_HOST_NAME: &str = "Verify native Docker host";
/// Builder creation step name.
pub(crate) const CREATE_IMAGE_BUILDER_NAME: &str = "Create task-owned Buildx builder";
/// Build step name.
pub(crate) const BUILD_IMAGE_NAME: &str = "Build OCI image";
/// Builder removal step name (always runs).
pub(crate) const REMOVE_IMAGE_BUILDER_NAME: &str = "Remove task-owned Buildx builder";
/// Layout upload step name.
pub(crate) const UPLOAD_IMAGE_NAME: &str = "Upload OCI layout";

const DOCKER: &str = "/usr/bin/docker --host unix:///var/run/docker.sock";
const DOCKER_ENV_RESET: &str = "unset DOCKER_CONFIG DOCKER_CONTEXT DOCKER_TLS_VERIFY DOCKER_CERT_PATH; DOCKER_HOST=unix:///var/run/docker.sock; export DOCKER_HOST";

/// One image plus the Dockerfile digests resolved from the checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageBuildPolicy {
    /// Validated declaration.
    pub image: ImageConfig,
    /// SHA-256 of the Dockerfile bytes per platform, read at planning time.
    pub dockerfile_sha256: BTreeMap<ImagePlatform, String>,
    /// Orchestrator-built steps that compile and stage the prebuilt artifact.
    ///
    /// Empty for a plain Dockerfile image. Their stack knowledge stays with
    /// the orchestrator; the renderer only orders and validates them.
    pub prebuilt_steps: Vec<Step>,
}

impl ImageBuildPolicy {
    /// True when the jobs run pinned tools (they then get Mise setup and the plan).
    #[must_use]
    pub fn uses_pinned_tools(&self) -> bool {
        !self.prebuilt_steps.is_empty()
    }

    /// Build job ids, one per platform in declaration order.
    #[must_use]
    pub fn job_ids(&self) -> Vec<String> {
        self.image
            .platforms
            .iter()
            .map(|platform| self.image.build_job_id(*platform))
            .collect()
    }

    /// Platform whose build job has this id.
    #[must_use]
    pub fn platform_for_job(&self, job_id: &str) -> Option<ImagePlatform> {
        self.image
            .platforms
            .iter()
            .copied()
            .find(|platform| self.image.build_job_id(*platform) == job_id)
    }

    fn dockerfile(&self, platform: ImagePlatform) -> Result<&str, RenderError> {
        self.image
            .dockerfile
            .for_platform(platform)
            .ok_or_else(|| invalid("image_dockerfile_platform_missing", &self.image.id))
    }

    fn validate(&self) -> Result<(), RenderError> {
        self.image
            .validate(".velnor/config.toml", 0)
            .map_err(RenderError::Contract)?;
        for platform in &self.image.platforms {
            let digest = self
                .dockerfile_sha256
                .get(platform)
                .ok_or_else(|| invalid("image_source_digest_missing", &self.image.id))?;
            let is_sha = digest.len() == 64
                && digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            if !is_sha {
                return Err(invalid("image_source_digest", &self.image.id));
            }
        }
        Ok(())
    }
}

/// Build the build job for one image and platform.
/// # Errors
pub fn build_image_build_job(
    policy: &ImageBuildPolicy,
    platform: ImagePlatform,
    checkout_uses: &str,
) -> Result<Job, RenderError> {
    policy.validate()?;
    let timeout = JobTimeout::new(policy.image.timeout_minutes).map_err(RenderError::Contract)?;
    Ok(Job {
        display_name: format!(
            "Build image {} ({})",
            policy.image.id,
            platform.oci_platform()
        ),
        runs_on: platform.runs_on().to_owned(),
        timeout_minutes: timeout,
        needs: vec![crate::render::PLAN_JOB_ID.to_owned()],
        condition: Some(crate::plan_gate::not_affected_gate(
            &policy.image.build_job_id(platform),
        )),
        permissions: Some(image_build_permissions()),
        cache_mode: None,
        environment: None,
        steps: image_build_steps(policy, platform, checkout_uses)?,
        check_runner: None,
    })
}

/// Reconstruct each declared job and reject altered, missing or split jobs.
/// # Errors
pub(crate) fn validate_image_build_jobs(
    jobs: &BTreeMap<String, Job>,
    policy: &ImageBuildPolicy,
    checkout_uses: &str,
    routing: &crate::workflow_routing::WorkflowRoutingPolicy,
) -> Result<Vec<String>, RenderError> {
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();
    for platform in &policy.image.platforms {
        let id = policy.image.build_job_id(*platform);
        if !seen.insert(id.clone()) {
            return Err(invalid("image_build_job_duplicate", &id));
        }
        let actual = jobs
            .get(&id)
            .ok_or_else(|| invalid("image_build_job_missing", &id))?;
        let expected = build_image_build_job(policy, *platform, checkout_uses)?;
        let mut expected_job = BTreeMap::from([(id.clone(), expected)]);
        crate::workflow_routing::route_jobs(&mut expected_job, routing)?;
        if routing.is_scale_set() {
            expected_job
                .get_mut(&id)
                .ok_or_else(|| invalid("image_build_job_missing", &id))?
                .display_name
                .push_str(SCALE_TASK_NAME_SUFFIX);
        }
        let expected = expected_job
            .get(&id)
            .ok_or_else(|| invalid("image_build_job_missing", &id))?;
        if actual.runs_on != platform.runs_on() && actual.runs_on != expected.runs_on {
            return Err(invalid("image_build_job_contract", &id));
        }
        let mut actual_job = BTreeMap::from([(id.clone(), actual.clone())]);
        crate::workflow_routing::route_jobs(&mut actual_job, routing)?;
        let actual = actual_job
            .get(&id)
            .ok_or_else(|| invalid("image_build_job_missing", &id))?;
        if !same_job_contract(actual, expected) {
            return Err(invalid("image_build_job_contract", &id));
        }
        ids.push(id);
    }
    Ok(ids)
}

fn image_build_steps(
    policy: &ImageBuildPolicy,
    platform: ImagePlatform,
    checkout_uses: &str,
) -> Result<Vec<Step>, RenderError> {
    let image = &policy.image;
    let mut remove = shell_step(
        REMOVE_IMAGE_BUILDER_NAME,
        bash(&remove_script(image, platform)),
        BTreeMap::new(),
    )?;
    remove.condition = Some("always()".to_owned());
    let mut upload = steps::action_step(
        UPLOAD_IMAGE_NAME,
        steps::UPLOAD_ARTIFACT_USES,
        BTreeMap::from([
            ("name".to_owned(), artifact_name(image, platform)),
            ("path".to_owned(), artifact_path(image, platform)),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            ("overwrite".to_owned(), "true".to_owned()),
            ("retention-days".to_owned(), "1".to_owned()),
        ]),
    )?;
    upload.condition = Some("success()".to_owned());
    let mut steps = vec![
        steps::checkout_step(checkout_uses)?,
        shell_step(
            VERIFY_IMAGE_SOURCE_NAME,
            bash(&source_script(policy, platform)?),
            BTreeMap::new(),
        )?,
        shell_step(
            VERIFY_IMAGE_HOST_NAME,
            bash(&host_script(platform)),
            BTreeMap::new(),
        )?,
    ];
    steps.extend(policy.prebuilt_steps.iter().cloned());
    steps.extend([
        shell_step(
            CREATE_IMAGE_BUILDER_NAME,
            bash(&create_script(image, platform)),
            BTreeMap::new(),
        )?,
        shell_step(
            BUILD_IMAGE_NAME,
            bash(&build_script(policy, platform)?),
            BTreeMap::new(),
        )?,
        remove,
        upload,
    ]);
    Ok(steps)
}

/// Artifact name carrying one platform's OCI layout.
#[must_use]
pub fn artifact_name(image: &ImageConfig, platform: ImagePlatform) -> String {
    format!("velnor-image-{}-{}", image.id, platform.job_suffix())
}

/// Expression-form directory holding the layout (`with: path:` only).
#[must_use]
pub fn artifact_path(image: &ImageConfig, platform: ImagePlatform) -> String {
    format!(
        "${{{{ runner.temp }}}}/velnor/images/{}/{}",
        image.id,
        platform.job_suffix()
    )
}

fn output_dir(image: &ImageConfig, platform: ImagePlatform) -> String {
    format!(
        "$RUNNER_TEMP/velnor/images/{}/{}",
        image.id,
        platform.job_suffix()
    )
}

fn builder_name(image: &ImageConfig, platform: ImagePlatform) -> String {
    format!(
        "velnor-${{GITHUB_RUN_ID}}-${{GITHUB_RUN_ATTEMPT}}-image-{}-{}",
        image.id,
        platform.job_suffix()
    )
}

const RUN_IDENTITY: &str =
    "[[ \"${GITHUB_RUN_ID:-}\" =~ ^[0-9]+$ ]]; [[ \"${GITHUB_RUN_ATTEMPT:-}\" =~ ^[0-9]+$ ]]";

fn source_script(
    policy: &ImageBuildPolicy,
    platform: ImagePlatform,
) -> Result<String, RenderError> {
    let dockerfile = policy.dockerfile(platform)?;
    let digest = policy
        .dockerfile_sha256
        .get(&platform)
        .ok_or_else(|| invalid("image_source_digest_missing", &policy.image.id))?;
    let mut lines = vec![
        "set -euo pipefail".to_owned(),
        "cd -P -- \"$GITHUB_WORKSPACE\"".to_owned(),
    ];
    for path in [dockerfile, policy.image.context.as_str()] {
        if path == "." {
            continue;
        }
        let parts = path.split('/').collect::<Vec<_>>();
        for count in 1..=parts.len() {
            lines.push(format!("test ! -L '{}'", parts[..count].join("/")));
        }
    }
    lines.push(format!("test -f '{dockerfile}'"));
    lines.push(format!("test -d '{}'", policy.image.context));
    lines.push(format!(
        "printf '%s  %s\\n' '{digest}' '{dockerfile}' | /usr/bin/sha256sum --check --status"
    ));
    Ok(lines.join("; "))
}

fn host_script(platform: ImagePlatform) -> String {
    let (machine, docker_arch) = match platform {
        ImagePlatform::LinuxAmd64 => ("x86_64", "linux/amd64"),
        ImagePlatform::LinuxArm64 => ("aarch64", "linux/arm64"),
    };
    format!(
        "set -euo pipefail; {DOCKER_ENV_RESET}; /usr/bin/uname -s | /usr/bin/grep -Fqx 'Linux'; /usr/bin/uname -m | /usr/bin/grep -Fqx '{machine}'; {RUN_IDENTITY}; {DOCKER} info --format '{{{{.OSType}}}}/{{{{.Architecture}}}}' | /usr/bin/grep -Fqx -e '{docker_arch}' -e 'linux/{machine}'; {DOCKER} buildx version"
    )
}

fn create_script(image: &ImageConfig, platform: ImagePlatform) -> String {
    format!(
        "set -euo pipefail; {DOCKER_ENV_RESET}; {RUN_IDENTITY}; builder=\"{}\"; {DOCKER} buildx create --name \"$builder\" --driver docker-container --driver-opt image={BUILDKIT_IMAGE} --bootstrap",
        builder_name(image, platform)
    )
}

fn build_script(policy: &ImageBuildPolicy, platform: ImagePlatform) -> Result<String, RenderError> {
    let image = &policy.image;
    let dockerfile = policy.dockerfile(platform)?;
    Ok(format!(
        "set -euo pipefail; {DOCKER_ENV_RESET}; {RUN_IDENTITY}; builder=\"{}\"; out=\"{}\"; mkdir -p \"$out\"; {DOCKER} buildx build --builder \"$builder\" --platform {} --file '{dockerfile}' --sbom=true --provenance=mode=max --metadata-file \"$out/metadata.json\" --output \"type=oci,dest=$out/image.tar\" -- '{}'; cd \"$out\"; /usr/bin/sha256sum image.tar > image.tar.sha256",
        builder_name(image, platform),
        output_dir(image, platform),
        platform.oci_platform(),
        image.context,
    ))
}

fn remove_script(image: &ImageConfig, platform: ImagePlatform) -> String {
    format!(
        "set -uo pipefail; {DOCKER_ENV_RESET}; {RUN_IDENTITY}; builder=\"{}\"; {DOCKER} buildx rm --force \"$builder\"",
        builder_name(image, platform)
    )
}

fn bash(script: &str) -> Vec<String> {
    vec!["bash".to_owned(), "-c".to_owned(), script.to_owned()]
}

fn invalid(code: &str, subject: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("{code}:{subject}"))
}

fn image_build_permissions() -> Permissions {
    Permissions {
        contents: PermissionLevel::Read,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::None,
        packages: PermissionLevel::None,
    }
}

fn same_job_contract(actual: &Job, expected: &Job) -> bool {
    actual.display_name == expected.display_name
        && actual.runs_on == expected.runs_on
        && actual.check_runner == expected.check_runner
        && actual.timeout_minutes == expected.timeout_minutes
        && actual.needs == expected.needs
        && actual.condition == expected.condition
        && actual.permissions == expected.permissions
        && actual.cache_mode == expected.cache_mode
        && actual.environment == expected.environment
        && actual.steps == expected.steps
}
