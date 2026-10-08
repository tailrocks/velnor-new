//! Paired hosted/Scale Set worker-topology assertions.

use std::path::Path;
use std::process::Command;

use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_routing::{job_body, required_file, workflow_config};
use velnor_actions_orchestrator_generation::generate::render_staged_tree;
use velnor_actions_orchestrator_generation::prepare::prepare;

const HOSTED_SOCKET: &str =
    "case \\\"${DOCKER_HOST:-}\\\" in\\n  \\\"\\\"|unix:///var/run/docker.sock) ;;";
const SCALE_SOCKET: &str = "test \\\"${DOCKER_HOST:-}\\\" = \\\"unix:///run/docker/docker.sock\\\"";
const CONTEXT_GUARD: &str = "test -z \\\"${DOCKER_CONTEXT:-}\\\"";
const HOSTED_DOCKER_HOST: &str = "docker --host unix:///var/run/docker.sock";
const SCALE_SET_DOCKER_HOST: &str = "docker --host unix:///run/docker/docker.sock";
const ALPINE: &str = "docker.io/library/alpine@sha256:3e9b4b680bfc9fb5269227cffbd6d42be39fbf7c0b908123913864aa4447e764";
const REDIS: &str = "docker.io/library/redis@sha256:ca0acbb137c1dc3339c8b147a58fd6f42775d4599327b50e7b116c23de501af2";

#[test]
fn topology_stages_are_paired_and_use_provider_specific_docker() -> TestResult {
    let yaml = render_qualification()?;
    for id in [
        "topology-runner-host",
        "topology-runner-host-hosted",
        "topology-job-container",
        "topology-job-container-hosted",
        "topology-docker-action",
        "topology-docker-action-hosted",
    ] {
        let body = job_body(&yaml, id)?;
        assert!(
            body.contains("if: inputs.mode == 'topology'"),
            "{id}: {body}"
        );
    }

    let scale_host = job_body(&yaml, "topology-runner-host")?;
    let hosted_host = job_body(&yaml, "topology-runner-host-hosted")?;
    assert!(scale_host.contains("runs-on: [velnor, ubuntu-26.04-scale-set]"));
    assert!(hosted_host.contains("runs-on: ubuntu-26.04"));
    assert!(scale_host.contains(SCALE_SOCKET), "{scale_host}");
    assert!(hosted_host.contains(HOSTED_SOCKET), "{hosted_host}");
    assert!(scale_host.contains(CONTEXT_GUARD), "{scale_host}");
    assert!(hosted_host.contains(CONTEXT_GUARD), "{hosted_host}");
    assert!(
        scale_host.contains(&format!("{SCALE_SET_DOCKER_HOST} info")),
        "{scale_host}"
    );
    assert!(
        hosted_host.contains(&format!("{HOSTED_DOCKER_HOST} info")),
        "{hosted_host}"
    );
    assert!(
        scale_host.contains(&format!("{SCALE_SET_DOCKER_HOST} pull")),
        "{scale_host}"
    );
    assert!(
        scale_host.contains(&format!("{SCALE_SET_DOCKER_HOST} run")),
        "{scale_host}"
    );
    assert!(
        hosted_host.contains(&format!("{HOSTED_DOCKER_HOST} pull")),
        "{hosted_host}"
    );
    assert!(
        hosted_host.contains(&format!("{HOSTED_DOCKER_HOST} run")),
        "{hosted_host}"
    );
    assert!(scale_host.contains(&format!("{SCALE_SET_DOCKER_HOST} run --rm --pull=never")));
    assert!(!scale_host.contains("docker run --rm"));
    assert!(!hosted_host.contains("docker run --rm"));
    assert_eq!(scale_host.matches(SCALE_SET_DOCKER_HOST).count(), 4);
    assert_eq!(hosted_host.matches(HOSTED_DOCKER_HOST).count(), 3);
    assert!(scale_host.contains("/var/run/docker.sock"), "{scale_host}");
    assert!(
        !hosted_host.contains("unix:///run/docker/docker.sock"),
        "{hosted_host}"
    );
    assert!(
        scale_host.contains("docker-workspace-mount-ok"),
        "{scale_host}"
    );
    assert!(
        hosted_host.contains("docker-workspace-mount-ok"),
        "{hosted_host}"
    );
    assert!(
        scale_host.contains("host-mode-localhost-ok"),
        "{scale_host}"
    );
    assert!(
        hosted_host.contains("host-mode-localhost-ok"),
        "{hosted_host}"
    );
    assert!(
        scale_host.contains("externals-read-only-ok"),
        "{scale_host}"
    );
    assert!(
        !hosted_host.contains("externals-read-only-ok"),
        "{hosted_host}"
    );

    let hosted_with_wrong_backend = hosted_host.replace(
        HOSTED_SOCKET,
        "case \\\"${DOCKER_HOST:-}\\\" in\\n  unix:///run/docker/docker.sock) ;;",
    );
    assert!(
        !uses_stock_hosted_socket(&hosted_with_wrong_backend),
        "the hosted contract must reject a DinD endpoint"
    );
    assert!(!uses_scale_set_socket(hosted_host));
    assert!(!uses_stock_hosted_socket(scale_host));
    Ok(())
}

#[test]
fn docker_context_override_is_rejected_by_both_provider_guards() -> TestResult {
    let yaml = render_qualification()?;
    for id in [
        "topology-runner-host",
        "topology-runner-host-hosted",
        "compose-scale-set",
        "compose-hosted",
    ] {
        let body = job_body(&yaml, id)?;
        let guard = shell_context_guard(body)?;
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

#[test]
fn compose_case_is_paired_pinned_and_cleans_up_after_failure() -> TestResult {
    let yaml = render_qualification()?;
    let hosted = job_body(&yaml, "compose-hosted")?;
    let scale = job_body(&yaml, "compose-scale-set")?;
    let compose_file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("qualification/compose/stack.yml");
    let compose = std::fs::read_to_string(compose_file)?;

    assert!(hosted.contains("runs-on: ubuntu-26.04"), "{hosted}");
    assert!(
        scale.contains("runs-on: [velnor, ubuntu-26.04-scale-set]"),
        "{scale}"
    );
    assert!(compose.contains(ALPINE), "{compose}");
    assert!(compose.contains(REDIS), "{compose}");
    assert_eq!(compose.matches("    image:").count(), 2, "{compose}");
    assert!(!compose.contains("redis:7-alpine"), "{compose}");
    assert!(!compose.contains("alpine:3.22"), "{compose}");

    for (id, body, endpoint, provider_step, forbidden_endpoint) in [
        (
            "hosted",
            hosted,
            "unix:///var/run/docker.sock",
            "Require GitHub-hosted stock Docker",
            "unix:///run/docker/docker.sock",
        ),
        (
            "scale-set",
            scale,
            "unix:///run/docker/docker.sock",
            "Require Velnor private DinD socket",
            "unix:///var/run/docker.sock",
        ),
    ] {
        let provider = body
            .find(provider_step)
            .ok_or_else(|| format!("{id}: missing provider guard"))?;
        let up = body
            .find("Start compose")
            .ok_or_else(|| format!("{id}: missing compose start"))?;
        let proof = body
            .find("Prove both services")
            .ok_or_else(|| format!("{id}: missing compose proof"))?;
        let cleanup = body
            .find("Remove compose")
            .ok_or_else(|| format!("{id}: missing compose cleanup"))?;
        assert!(
            provider < up && up < proof && proof < cleanup,
            "{id}: {body}"
        );
        assert!(
            body.contains(&format!("docker --host {endpoint} compose")),
            "{id}: {body}"
        );
        let cleanup_step = &body[cleanup..];
        assert!(
            cleanup_step.contains("if: always()"),
            "{id}: {cleanup_step}"
        );
        assert!(
            cleanup_step.contains(&format!("docker --host {endpoint} compose --project-name")),
            "{id}: {cleanup_step}"
        );
        assert!(
            cleanup_step.contains("down --volumes"),
            "{id}: {cleanup_step}"
        );
        assert!(!body.contains(forbidden_endpoint), "{id}: {body}");
        assert!(
            !body.contains("docker compose -f"),
            "{id}: unpinned client endpoint: {body}"
        );
    }
    Ok(())
}

#[test]
fn job_container_and_docker_action_workloads_are_paired() -> TestResult {
    let yaml = render_qualification()?;
    let scale_container = job_body(&yaml, "topology-job-container")?;
    let hosted_container = job_body(&yaml, "topology-job-container-hosted")?;
    for body in [scale_container, hosted_container] {
        assert!(body.contains(ALPINE), "{body}");
        assert!(body.contains(&format!("image: {REDIS}")), "{body}");
        assert!(body.contains("busybox nc -w 3 redis 6379"), "{body}");
        assert!(body.contains("job-container-work-volume-ok"), "{body}");
        assert!(body.contains("service-alias-ok"), "{body}");
        assert!(!body.contains("49327:6379"), "{body}");
    }
    assert!(has_dependency(scale_container, "topology-runner-host"));
    assert!(has_dependency(
        hosted_container,
        "topology-runner-host-hosted"
    ));

    let scale_action = job_body(&yaml, "topology-docker-action")?;
    let hosted_action = job_body(&yaml, "topology-docker-action-hosted")?;
    for body in [scale_action, hosted_action] {
        assert!(
            body.contains("uses: ./qualification/actions/docker"),
            "{body}"
        );
        assert!(body.contains("VELNOR_TOPOLOGY_MARKER:"), "{body}");
        assert!(
            body.contains("VELNOR_TOPOLOGY_MARKER: .velnor-topology.txt"),
            "{body}"
        );
        assert!(body.contains("docker-action-work-volume-ok"), "{body}");
    }
    assert!(has_dependency(scale_action, "topology-job-container"));
    assert!(has_dependency(
        hosted_action,
        "topology-job-container-hosted"
    ));
    assert!(scale_action.contains(SCALE_SOCKET), "{scale_action}");
    assert!(hosted_action.contains(HOSTED_SOCKET), "{hosted_action}");
    assert!(scale_action.contains("DOCKER_HOST: unix:///run/docker/docker.sock"));
    assert!(hosted_action.contains("DOCKER_HOST: unix:///var/run/docker.sock"));
    assert!(scale_action.contains("DOCKER_CONTEXT: \"\""));
    assert!(hosted_action.contains("DOCKER_CONTEXT: \"\""));
    assert!(!hosted_action.contains("unix:///run/docker/docker.sock"));
    assert!(!scale_action.contains("runs-on: ubuntu-26.04\n"));
    Ok(())
}

#[test]
fn rendered_workflow_matches_committed_generated_file() -> TestResult {
    let yaml = render_qualification()?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let committed = std::fs::read_to_string(root.join(".github/workflows/qualification.yml"))?;
    assert_eq!(committed, yaml);
    Ok(())
}

fn render_qualification() -> Result<String, Box<dyn std::error::Error>> {
    let repo = make_repo(&workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    Ok(required_file(&tree, ".github/workflows/qualification.yml")?.to_owned())
}

fn uses_stock_hosted_socket(body: &str) -> bool {
    body.contains(HOSTED_SOCKET)
        && body.contains(CONTEXT_GUARD)
        && body.contains("test -S /var/run/docker.sock")
        && body.contains(&format!("{HOSTED_DOCKER_HOST} info >/dev/null"))
        && !body.contains("unix:///run/docker/docker.sock")
}

fn has_dependency(body: &str, parent: &str) -> bool {
    body.contains(&format!("needs:\n      - {parent}"))
}

fn uses_scale_set_socket(body: &str) -> bool {
    body.contains(SCALE_SOCKET)
        && body.contains(CONTEXT_GUARD)
        && body.contains("test -S /run/docker/docker.sock")
        && body.contains("test ! -e /var/run/docker.sock")
        && body.contains(&format!("{SCALE_SET_DOCKER_HOST} info >/dev/null"))
}

fn shell_context_guard(body: &str) -> Result<String, Box<dyn std::error::Error>> {
    if !body.contains(CONTEXT_GUARD) {
        return Err("provider step is missing the Docker context guard".into());
    }
    Ok(CONTEXT_GUARD.replace("\\\"", "\""))
}
