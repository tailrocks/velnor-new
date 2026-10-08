//! Paired Buildx context, backend, and imported-cache proof assertions.

use std::path::Path;
use std::process::Command;

use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_routing::{job_body, required_file, workflow_config};
use velnor_actions_orchestrator_generation::generate::render_staged_tree;
use velnor_actions_orchestrator_generation::prepare::prepare;

const BUILDKIT_IMAGE: &str = "docker.io/moby/buildkit@sha256:98cc6a3fc46220d00f8224ae483f3274fc874e9be8d7dd1e2e2c5481209228b5";
const HOSTED_ENDPOINT: &str = "unix:///var/run/docker.sock";
const SCALE_SET_ENDPOINT: &str = "unix:///run/docker/docker.sock";

#[test]
fn buildx_case_pins_context_and_provider_route() -> TestResult {
    let yaml = render_qualification()?;
    let hosted = job_body(&yaml, "buildx-hosted")?;
    let scale = job_body(&yaml, "buildx-scale-set")?;
    let context =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../qualification/buildx-context");
    let dockerfile = std::fs::read_to_string(context.join("Dockerfile"))?;
    let payload = std::fs::read_to_string(context.join("payload.txt"))?;

    assert_eq!(dockerfile, "FROM scratch\nCOPY payload.txt /payload.txt\n");
    assert_eq!(payload, "velnor-g4-buildx-cache-probe-v1\n");
    assert_buildx_job("hosted", hosted, "runs-on: ubuntu-26.04", HOSTED_ENDPOINT);
    assert_buildx_job(
        "scale-set",
        scale,
        "runs-on: [velnor, ubuntu-26.04-scale-set]",
        SCALE_SET_ENDPOINT,
    );

    assert!(!hosted.contains(SCALE_SET_ENDPOINT), "{hosted}");
    assert!(!scale.contains(HOSTED_ENDPOINT), "{scale}");
    let hosted_probe = run_value(hosted, "Buildx pinned context/cache probe")?;
    let scale_probe = run_value(scale, "Buildx pinned context/cache probe")?;
    assert_eq!(
        hosted_probe.replace(HOSTED_ENDPOINT, "<provider-endpoint>"),
        scale_probe.replace(SCALE_SET_ENDPOINT, "<provider-endpoint>")
    );
    Ok(())
}

fn assert_buildx_job(id: &str, body: &str, selector: &str, endpoint: &str) {
    for expected in [
        "if: inputs.mode == 'features' || inputs.mode == 'buildx'",
        selector,
        "Buildx pinned context/cache probe",
        "Remove Buildx builder and local cache",
        "if: always()",
        &format!("endpoint=\\\"{endpoint}\\\""),
        BUILDKIT_IMAGE,
        "--driver docker-container",
        "--platform linux/amd64",
        "cache-hit.mjs",
        "--self-test",
        "second-build.log",
    ] {
        assert!(body.contains(expected), "{id}: missing {expected}: {body}");
    }
    for expected in [
        "buildx prune --builder \\\"$builder\\\" --all --force",
        "buildx rm \\\"$builder\\\"",
        "buildx ls --format '{{.Name}}'",
    ] {
        assert!(body.contains(expected), "{id}: missing {expected}: {body}");
    }
    assert!(!body.contains("docker buildx"), "{id}: {body}");
    assert!(!body.contains("--push"), "{id}: {body}");
    assert!(!body.contains("DOCKER_CONTEXT: "), "{id}: {body}");
}

#[test]
fn buildx_cache_assertion_uses_exact_copy_vertex_and_synthetic_controls() -> TestResult {
    let context =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../qualification/buildx-context");
    let parser = std::fs::read_to_string(context.join("cache-hit.mjs"))?;
    assert!(parser.contains("#5 [2/2] COPY payload.txt /payload.txt"));
    assert!(parser.contains("#5 CACHED"));
    assert!(parser.contains("#8 CACHED"));
    assert!(parser.contains("noncachedCopy"));
    let yaml = render_qualification()?;
    for id in ["buildx-hosted", "buildx-scale-set"] {
        let body = job_body(&yaml, id)?;
        assert!(body.contains("cache-hit.mjs"), "{id}: {body}");
        assert!(body.contains("--self-test"), "{id}: {body}");
    }
    Ok(())
}

#[test]
fn buildx_context_override_is_rejected_by_both_provider_guards() -> TestResult {
    let yaml = render_qualification()?;
    for id in ["buildx-hosted", "buildx-scale-set"] {
        let body = job_body(&yaml, id)?;
        let guard = crate::impl_schema2_topology::shell_context_guard(body)?;
        let rejected = Command::new("bash")
            .arg("-c")
            .arg(&guard)
            .env("DOCKER_CONTEXT", "unapproved-context")
            .status()?;
        assert!(
            !rejected.success(),
            "{id} accepted a Docker context override"
        );
        let accepted = Command::new("bash")
            .arg("-c")
            .arg(&guard)
            .env_remove("DOCKER_CONTEXT")
            .status()?;
        assert!(accepted.success(), "{id} rejected the normal context state");
    }
    Ok(())
}

fn render_qualification() -> Result<String, Box<dyn std::error::Error>> {
    let repo = make_repo(&workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    Ok(required_file(&tree, ".github/workflows/qualification.yml")?.to_owned())
}

fn run_value<'a>(body: &'a str, name: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    let step = body
        .find(&format!("- name: {name}"))
        .ok_or_else(|| format!("missing step {name}"))?;
    let run = body[step..]
        .find("run: ")
        .map(|offset| step + offset + "run: ".len())
        .ok_or_else(|| format!("missing run field for {name}"))?;
    Ok(body[run..]
        .lines()
        .next()
        .ok_or_else(|| format!("missing run value for {name}"))?)
}
