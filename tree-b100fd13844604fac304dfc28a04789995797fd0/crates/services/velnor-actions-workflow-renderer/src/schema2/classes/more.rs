//! Qualification modes outside the first class set. Each dispatches on its
//! own `inputs.mode` and is not selected by `features`.

use super::super::features::{
    CHECKOUT_USES, checkout_step, finish, gated, lane_base, redis_service, run_step,
};
use super::steps::uses_with;
use super::{Extras, RunnerSpec, both};
use velnor_actions_workflow_tree::yaml::Yaml;

const COMPOSE_UP: &str = "docker compose -f qualification/compose/stack.yml up -d --wait";
const COMPOSE_PROOF: &str = "docker compose -f qualification/compose/stack.yml ps --services --status running > \"$RUNNER_TEMP/g4-compose-ps\" && grep -qx api \"$RUNNER_TEMP/g4-compose-ps\" && grep -qx db \"$RUNNER_TEMP/g4-compose-ps\"";
const COMPOSE_DOWN: &str = "docker compose -f qualification/compose/stack.yml down --volumes";
const BIND_RUN: &str = "printf '%s\\n' bind-ok > \"$GITHUB_WORKSPACE/g4-bind.txt\" && docker run --rm -v \"$GITHUB_WORKSPACE/g4-bind.txt:/g4-bind.txt:ro\" alpine:3.22 cat /g4-bind.txt > \"$RUNNER_TEMP/g4-bind-out\" && grep -qx bind-ok \"$RUNNER_TEMP/g4-bind-out\"";
const SERVICE_PROBE: &str = "i=0; while [ \"$i\" -lt 30 ]; do nc -z -w 1 127.0.0.1 6379 && break; i=$((i+1)); sleep 1; done; nc -z -w 1 127.0.0.1 6379 && echo service-up && sleep 900";
const TC_RUN: &str =
    "npm ci --prefix qualification/testcontainers && node qualification/testcontainers/reap.mjs";
const SUBMODULE_PROOF: &str = "git rev-parse HEAD > \"$RUNNER_TEMP/g4-head\" && grep -qx \"$GITHUB_SHA\" \"$RUNNER_TEMP/g4-head\" && grep -qx submodule-ok qualification/fixtures/submodule/MARKER && grep -qx lfs-ok qualification/fixtures/lfs-marker.txt";
const PORT_HOLD: &str = "docker run -d --name g4-hold -p 8080:80 alpine:3.22 sleep 120 && i=0 && while [ \"$i\" -lt 30 ]; do docker port g4-hold 80 | grep -q 8080 && break; i=$((i+1)); sleep 1; done && docker port g4-hold 80 | grep -q 8080 && echo port-held && sleep 45 && docker rm -f g4-hold";
const PRESSURE_RUN: &str = "echo pressure-start && sleep 150 && echo pressure-ok";

/// Compose, bind, cancel-service, testcontainers, submodule, ports, and pressure.
pub(super) fn jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    let mut out = paired(hosted, scale);
    out.extend(testcontainers_jobs(hosted, scale));
    out.extend(submodule_jobs(hosted, scale));
    out.extend(ports_jobs(hosted, scale));
    out.extend(pressure_jobs(hosted, scale));
    out.extend(secret_jobs(hosted, scale));
    out
}

/// Holds a GitHub Actions secret in the step environment. The run script
/// checks that it is non-empty and does not print it.
fn secret_jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    both(
        "secret",
        "Secret",
        hosted,
        scale,
        secret_steps(),
        Extras::default(),
    )
}

fn secret_steps() -> Vec<Yaml> {
    vec![super::steps::run_env(
        "Hold secret canary",
        &[("G3_CANARY", "${{ secrets.G3_CANARY }}")],
        "test -n \"$G3_CANARY\" && sleep 180",
    )]
}

fn paired(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    let mut out = Vec::new();
    out.extend(both(
        "compose",
        "Compose",
        hosted,
        scale,
        compose_steps(),
        Extras::default(),
    ));
    out.extend(both(
        "bind",
        "Bind",
        hosted,
        scale,
        bind_steps(),
        Extras::default(),
    ));
    out.extend(both(
        "cancel-service",
        "Cancel service",
        hosted,
        scale,
        cancel_service_steps(),
        service_extras(),
    ));
    out
}

fn submodule_jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    both(
        "submodule",
        "Submodule",
        hosted,
        scale,
        submodule_steps(),
        Extras::default(),
    )
}

fn testcontainers_jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    let steps = testcontainers_steps();
    vec![
        timed(
            "testcontainers-hosted",
            "Testcontainers / GitHub hosted",
            "testcontainers",
            hosted,
            steps.clone(),
        ),
        timed(
            "testcontainers-scale-set",
            "Testcontainers / Velnor Scale Set",
            "testcontainers",
            scale,
            steps,
        ),
    ]
}

fn ports_jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    let steps = ports_steps();
    vec![
        timed(
            "ports-a-hosted",
            "Ports A / GitHub hosted",
            "ports",
            hosted,
            steps.clone(),
        ),
        timed(
            "ports-b-hosted",
            "Ports B / GitHub hosted",
            "ports",
            hosted,
            steps.clone(),
        ),
        timed(
            "ports-a-scale-set",
            "Ports A / Velnor Scale Set",
            "ports",
            scale,
            steps.clone(),
        ),
        timed(
            "ports-b-scale-set",
            "Ports B / Velnor Scale Set",
            "ports",
            scale,
            steps,
        ),
    ]
}

fn pressure_jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    let steps = pressure_steps();
    vec![
        timed(
            "pressure-a-hosted",
            "Pressure A / GitHub hosted",
            "pressure",
            hosted,
            steps.clone(),
        ),
        timed(
            "pressure-b-hosted",
            "Pressure B / GitHub hosted",
            "pressure",
            hosted,
            steps.clone(),
        ),
        timed(
            "pressure-c-hosted",
            "Pressure C / GitHub hosted",
            "pressure",
            hosted,
            steps.clone(),
        ),
        timed(
            "pressure-a-scale-set",
            "Pressure A / Velnor Scale Set",
            "pressure",
            scale,
            steps.clone(),
        ),
        timed(
            "pressure-b-scale-set",
            "Pressure B / Velnor Scale Set",
            "pressure",
            scale,
            steps.clone(),
        ),
        timed(
            "pressure-c-scale-set",
            "Pressure C / Velnor Scale Set",
            "pressure",
            scale,
            steps,
        ),
    ]
}

fn timed(
    id: &str,
    name: &str,
    mode: &str,
    runner: &RunnerSpec,
    steps: Vec<Yaml>,
) -> (String, Yaml) {
    let when = format!("inputs.mode == '{mode}'");
    gated(finish(id, lane_base(name, runner, 30), steps), &when)
}

fn service_extras() -> Extras {
    Extras {
        services: Some(redis_service()),
        ..Extras::default()
    }
}

fn compose_steps() -> Vec<Yaml> {
    vec![
        checkout_step(),
        run_step("Start compose", COMPOSE_UP),
        run_step("Prove both services", COMPOSE_PROOF),
        run_step("Remove compose", COMPOSE_DOWN),
        run_step("Record compose", "echo compose-ok"),
    ]
}

fn bind_steps() -> Vec<Yaml> {
    vec![run_step("Bind workspace file", BIND_RUN)]
}

fn cancel_service_steps() -> Vec<Yaml> {
    vec![run_step("Probe service then sleep", SERVICE_PROBE)]
}

fn testcontainers_steps() -> Vec<Yaml> {
    vec![checkout_step(), run_step("Install and reap", TC_RUN)]
}

fn submodule_steps() -> Vec<Yaml> {
    vec![
        uses_with(
            "Check out",
            CHECKOUT_USES,
            &[
                ("submodules", "recursive"),
                ("lfs", "true"),
                ("persist-credentials", "false"),
            ],
        ),
        run_step("Prove submodule and LFS", SUBMODULE_PROOF),
    ]
}

fn ports_steps() -> Vec<Yaml> {
    vec![run_step("Hold host port", PORT_HOLD)]
}

fn pressure_steps() -> Vec<Yaml> {
    vec![run_step("Pressure sleep", PRESSURE_RUN)]
}
