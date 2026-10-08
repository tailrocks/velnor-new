//! Paired hosted and Velnor worker-topology cases.
//!
//! Hosted jobs must use GitHub's stock Docker socket. Scale Set jobs must use
//! the runner's private `DinD` socket. The workload checks stay paired while
//! each provider's orchestration contract remains explicit.

use super::super::docker::docker_provider_guard;
pub(super) use super::super::docker::{docker_endpoint, docker_socket_path};
use super::super::features::{
    checkout_step, lane_base, lane_base_with_container, local_action_step, run_step,
};
use super::super::workflows::with_if;
use super::super::{RunnerLane, RunnerSpec};
use super::DOCKER_ACTION;
use velnor_actions_workflow_tree::job_entries::finish;
use velnor_actions_workflow_tree::yaml::Yaml;

const MODE: &str = "inputs.mode == 'topology'";
const ALPINE: &str = "docker.io/library/alpine@sha256:3e9b4b680bfc9fb5269227cffbd6d42be39fbf7c0b908123913864aa4447e764";
pub(super) const REDIS: &str = "docker.io/library/redis@sha256:ca0acbb137c1dc3339c8b147a58fd6f42775d4599327b50e7b116c23de501af2";
pub(super) const TESTCONTAINERS_RYUK: &str = "docker.io/testcontainers/ryuk@sha256:f0456560ea5b4acdbed0da0efc33b5f9dd6bc1e59f2337106826dcb5b0b0e981";
const REDIS_OPTIONS: &str =
    "--health-cmd \"redis-cli ping\" --health-interval 5s --health-timeout 5s --health-retries 12";
const HOST_WORKSPACE_AND_SERVICE: &str = r#"set -euo pipefail
marker=".velnor-topology.txt"
printf '%s\n' runner-work-volume-ok > "$GITHUB_WORKSPACE/$marker"
test -w "$GITHUB_WORKSPACE"
probe_redis() {
  local response
  exec 3<>/dev/tcp/127.0.0.1/49327 || return 1
  printf 'PING\r\n' >&3 || return 1
  IFS= read -r -t 2 response <&3 || return 1
  exec 3>&-
  test "${response%$'\r'}" = "+PONG"
}
ready=0
deadline=$((SECONDS + 60))
while [ "$SECONDS" -lt "$deadline" ]; do
  if probe_redis; then ready=1; break; fi
  sleep 1
done
test "$ready" -eq 1
printf '%s\n' 'runner-workspace-ok' 'host-mode-localhost-ok'"#;

const DOCKER_WORKSPACE: &str = r#"set -euo pipefail
alpine='docker.io/library/alpine@sha256:3e9b4b680bfc9fb5269227cffbd6d42be39fbf7c0b908123913864aa4447e764'
docker --host {endpoint} pull "$alpine" >/dev/null
docker --host {endpoint} run --rm --pull=never --network=none \
  --mount "type=bind,src=$GITHUB_WORKSPACE,dst=/probe,readonly" \
  --entrypoint /bin/sh "$alpine" -ec \
  'grep -qx runner-work-volume-ok /probe/.velnor-topology.txt'
printf '%s\n' 'docker-workspace-mount-ok'"#;

const EXTERNALS_READ_ONLY: &str = r"set -euo pipefail
docker --host unix:///run/docker/docker.sock run --rm --pull=never --network=none \
  --mount type=bind,src=/home/runner/externals,dst=/probe,readonly \
  --entrypoint /bin/sh docker.io/library/alpine@sha256:3e9b4b680bfc9fb5269227cffbd6d42be39fbf7c0b908123913864aa4447e764 -ec \
  'test -d /probe && ls -A /probe | grep -q .; if : > /probe/.velnor-external-write-probe 2>/dev/null; then rm -f /probe/.velnor-external-write-probe; exit 72; fi'
printf '%s\n' 'externals-read-only-ok'";

const JOB_CONTAINER_PROBE: &str = r#"set -eu
marker="$GITHUB_WORKSPACE/.velnor-container.txt"
printf '%s\n' job-container-work-volume-ok > "$marker"
test -w "$GITHUB_WORKSPACE"
test "$(cat "$marker")" = job-container-work-volume-ok
response="$(printf 'PING\r\n' | busybox nc -w 3 redis 6379 | sed -n '1p' | tr -d '\r')"
test "$response" = '+PONG'
printf '%s\n' 'job-container-workspace-ok' 'service-alias-ok'"#;

/// Render the three paired topology stages after the other qualification modes.
pub(crate) fn jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    vec![
        gated(
            runner_host(
                "topology-runner-host",
                "Worker topology / runner host namespace",
                scale,
            ),
            MODE,
        ),
        gated(
            runner_host(
                "topology-runner-host-hosted",
                "Worker topology / runner host namespace / GitHub hosted",
                hosted,
            ),
            MODE,
        ),
        gated(
            job_container(
                "topology-job-container",
                "Worker topology / job container and service alias",
                scale,
                "topology-runner-host",
            ),
            MODE,
        ),
        gated(
            job_container(
                "topology-job-container-hosted",
                "Worker topology / job container and service alias / GitHub hosted",
                hosted,
                "topology-runner-host-hosted",
            ),
            MODE,
        ),
        gated(
            docker_action(
                "topology-docker-action",
                "Worker topology / external and Docker actions",
                scale,
                "topology-job-container",
            ),
            MODE,
        ),
        gated(
            docker_action(
                "topology-docker-action-hosted",
                "Worker topology / external and Docker actions / GitHub hosted",
                hosted,
                "topology-job-container-hosted",
            ),
            MODE,
        ),
    ]
}

fn runner_host(id: &str, name: &str, runner: &RunnerSpec) -> (String, Yaml) {
    let mut fields = lane_base(name, runner, 20);
    fields.push(("services".to_owned(), redis_service(true)));
    let mut steps = vec![
        docker_provider_step(runner),
        checkout_step(),
        run_step(
            "Check runner workspace and host-mode service",
            HOST_WORKSPACE_AND_SERVICE,
        ),
        run_step(
            "Check Docker workspace bind",
            &docker_workspace_probe(runner),
        ),
    ];
    if runner.lane == RunnerLane::ScaleSet {
        steps.push(run_step(
            "Check read-only runner externals",
            EXTERNALS_READ_ONLY,
        ));
    }
    finish(id, fields, steps)
}

fn job_container(id: &str, name: &str, runner: &RunnerSpec, parent: &str) -> (String, Yaml) {
    let mut fields = lane_base_with_container(name, runner, 20, true);
    fields.push(("needs".to_owned(), Yaml::Seq(vec![Yaml::str(parent)])));
    fields.push(("container".to_owned(), Yaml::str(ALPINE)));
    fields.push(("services".to_owned(), redis_service(false)));
    finish(
        id,
        fields,
        vec![
            checkout_step(),
            run_step(
                "Check job-container workspace and service DNS",
                JOB_CONTAINER_PROBE,
            ),
        ],
    )
}

fn docker_action(id: &str, name: &str, runner: &RunnerSpec, parent: &str) -> (String, Yaml) {
    let mut fields = lane_base(name, runner, 20);
    fields.push(("needs".to_owned(), Yaml::Seq(vec![Yaml::str(parent)])));
    let mut action = local_action_step("Local Docker action and workspace mount", DOCKER_ACTION);
    if let Yaml::Map(entries) = &mut action {
        entries.push((
            "env".to_owned(),
            Yaml::Map(vec![
                (
                    "VELNOR_TOPOLOGY_MARKER".to_owned(),
                    Yaml::str(".velnor-topology.txt"),
                ),
                ("DOCKER_HOST".to_owned(), Yaml::str(docker_endpoint(runner))),
                ("DOCKER_CONTEXT".to_owned(), Yaml::str("")),
            ]),
        ));
    }
    finish(
        id,
        fields,
        vec![
            docker_provider_step(runner),
            checkout_step(),
            run_step(
                "Seed Docker-action workspace probe",
                "printf '%s\\n' docker-action-work-volume-ok > \"$GITHUB_WORKSPACE/.velnor-topology.txt\"",
            ),
            action,
        ],
    )
}

fn docker_workspace_probe(runner: &RunnerSpec) -> String {
    DOCKER_WORKSPACE.replace("{endpoint}", docker_endpoint(runner))
}

pub(super) fn docker_provider_step(runner: &RunnerSpec) -> Yaml {
    let (name, command) = docker_provider_guard(runner);
    run_step(name, command)
}

fn redis_service(publish_port: bool) -> Yaml {
    let mut redis = vec![
        ("image".to_owned(), Yaml::str(REDIS)),
        ("options".to_owned(), Yaml::str(REDIS_OPTIONS)),
    ];
    if publish_port {
        redis.push((
            "ports".to_owned(),
            Yaml::Seq(vec![Yaml::quoted("49327:6379")]),
        ));
    }
    Yaml::Map(vec![("redis".to_owned(), Yaml::Map(redis))])
}

fn gated(job: (String, Yaml), when: &str) -> (String, Yaml) {
    with_if(job, when)
}
