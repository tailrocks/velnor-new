//! Fixed ephemeral PostgreSQL preparation for typed Gradle fixture inputs.

use std::collections::BTreeMap;
use velnor_actions_contract::config::PostgresFixture;
use velnor_actions_contract::{Step, StepId, StepKind};
use velnor_actions_native::java::{DatabaseInitialization, FixtureValue, postgres_initialization};

use crate::OrchestratorError;

use velnor_actions_mise::catalog::POSTGRES_FIXTURE_IMAGE as IMAGE;
const STATE: &str = "${RUNNER_TEMP:?}/velnor-gradle-postgres";
const DOCKER: &str = "docker --host unix:///var/run/docker.sock";

/// Prepare a local fixture and return its mandatory always-run removal step.
pub(crate) fn steps(fixture: &PostgresFixture) -> Result<(Step, Step), OrchestratorError> {
    let initialization = postgres_initialization(fixture)?;
    let first = &initialization.initial_database;
    let environment_argv: Vec<String> = initialization
        .environment
        .iter()
        .flat_map(|(key, value)| ["--env".to_owned(), format!("{key}={value}")])
        .collect();
    let environment = velnor_actions_workflow_renderer::join_argv_for_run(&environment_argv)?;
    let mut script = format!(
        "set -euo pipefail; unset DOCKER_HOST DOCKER_CONTEXT DOCKER_TLS_VERIFY \
         DOCKER_CERT_PATH DOCKER_API_VERSION DOCKER_CUSTOM_HEADERS; \
         umask 077; state=\"{STATE}\"; \
         test ! -e \"$state\"; mkdir -m 700 \"$state\"; \
         mkdir -m 700 \"$state/config\"; export DOCKER_CONFIG=\"$state/config\"; \
         cid=; cleanup() {{ if test -n \"$cid\"; then {remove} fi; }}; trap cleanup EXIT; \
         cid=$({DOCKER} run --detach --publish 127.0.0.1::5432 \
         --label \"io.velnor.fixture.kind=postgres\" \
         --label \"io.velnor.fixture.run=${{VELNOR_FIXTURE_RUN:?}}\" \
         --label \"io.velnor.fixture.attempt=${{VELNOR_FIXTURE_ATTEMPT:?}}\" \
         --label \"io.velnor.fixture.job=${{VELNOR_FIXTURE_JOB:?}}\" \
         --tmpfs /var/lib/postgresql:rw,nosuid,nodev,size=1073741824 \
         {environment} '{IMAGE}'); [[ \"$cid\" =~ ^[a-f0-9]{{64}}$ ]]; \
         printf 'cid=%s\\n' \"$cid\" >> \"${{GITHUB_OUTPUT:?}}\"; ready=0; \
         for attempt in {{1..60}}; do if {DOCKER} exec \"$cid\" \
         pg_isready --host 127.0.0.1 --username '{user}' --dbname '{first}'; \
         then ready=1; break; fi; sleep 1; done; test \"$ready\" = 1; ",
        user = fixture.user,
        remove = remove_container(),
    );
    for argv in &initialization.create_database_argv {
        let argv = velnor_actions_workflow_renderer::join_argv_for_run(argv)?;
        script.push_str(&format!("{DOCKER} exec \"$cid\" {argv}; "));
    }
    script.push_str(&format!(
        "port=$({DOCKER} inspect --format \
         '{{{{(index (index .NetworkSettings.Ports \"5432/tcp\") 0).HostPort}}}}' \
         \"$cid\"); [[ \"$port\" =~ ^[0-9]{{1,5}}$ ]]; \
         (( port > 0 && port <= 65535 )); "
    ));
    append_bindings(&mut script, &initialization);
    script.push_str("trap - EXIT");
    let mut prepare = step("Prepare local PostgreSQL fixture", script)?;
    prepare.id = Some(StepId::new("velnor_postgres_fixture")?);
    let mut cleanup = step("Remove local PostgreSQL fixture", cleanup_script())?;
    cleanup.condition = Some("always()".to_owned());
    if let StepKind::Shell { env, .. } = &mut cleanup.kind {
        env.insert(
            "VELNOR_FIXTURE_CID".to_owned(),
            "${{ steps.velnor_postgres_fixture.outputs.cid }}".to_owned(),
        );
    }
    Ok((prepare, cleanup))
}

fn append_bindings(script: &mut String, initialization: &DatabaseInitialization) {
    for binding in &initialization.bindings {
        let key = &binding.name;
        let value = match &binding.value {
            FixtureValue::Literal(value) => value.clone(),
            FixtureValue::Port => "$port".to_owned(),
            FixtureValue::JdbcUrl(database) => {
                format!("jdbc:postgresql://127.0.0.1:$port/{database}")
            }
        };
        script.push_str(&format!(
            "printf '%s=%s\\n' '{key}' \"{value}\" >> \"${{GITHUB_ENV:?}}\"; "
        ));
    }
}

fn cleanup_script() -> String {
    format!(
        "set -euo pipefail; unset DOCKER_HOST DOCKER_CONTEXT DOCKER_TLS_VERIFY \
         DOCKER_CERT_PATH DOCKER_API_VERSION DOCKER_CUSTOM_HEADERS; state=\"{STATE}\"; \
         config=$(mktemp -d \"${{RUNNER_TEMP:?}}/velnor-postgres-cleanup.XXXXXXXX\"); \
         trap 'rm -rf -- \"$config\"' EXIT; export DOCKER_CONFIG=\"$config\"; \
         cid=${{VELNOR_FIXTURE_CID:-}}; if test -n \"$cid\"; then \
         [[ \"$cid\" =~ ^[a-f0-9]{{64}}$ ]]; {remove} fi; rm -rf -- \"$state\"",
        remove = remove_container(),
    )
}

fn remove_container() -> String {
    format!(
        "[[ \"$cid\" =~ ^[a-f0-9]{{64}}$ ]]; present=$({DOCKER} ps --all --quiet --no-trunc --filter \"id=$cid\"); \
         if test -n \"$present\"; then test \"$present\" = \"$cid\"; \
         ownership=$({DOCKER} inspect --format \
         '{{{{.Config.Image}}}}|{{{{index .Config.Labels \"io.velnor.fixture.kind\"}}}}|\
         {{{{index .Config.Labels \"io.velnor.fixture.run\"}}}}|\
         {{{{index .Config.Labels \"io.velnor.fixture.attempt\"}}}}|\
         {{{{index .Config.Labels \"io.velnor.fixture.job\"}}}}' \"$cid\"); \
         test \"$ownership\" = \
         \"{IMAGE}|postgres|${{VELNOR_FIXTURE_RUN:?}}|${{VELNOR_FIXTURE_ATTEMPT:?}}|${{VELNOR_FIXTURE_JOB:?}}\"; \
         if ! {DOCKER} rm --force --volumes \"$cid\"; then \
         present=$({DOCKER} ps --all --quiet --no-trunc --filter \"id=$cid\"); \
         test -z \"$present\"; fi; fi; "
    )
}

fn step(name: &str, script: String) -> Result<Step, OrchestratorError> {
    let mut env = [
        "BASH_ENV",
        "ENV",
        "LD_PRELOAD",
        "LD_LIBRARY_PATH",
        "DYLD_INSERT_LIBRARIES",
        "DYLD_LIBRARY_PATH",
    ]
    .into_iter()
    .map(|key| (key.to_owned(), String::new()))
    .collect::<BTreeMap<_, _>>();
    env.insert("PATH".to_owned(), "/usr/bin:/bin".to_owned());
    for (key, value) in [
        ("RUN", "run_id"),
        ("ATTEMPT", "run_attempt"),
        ("JOB", "job"),
    ] {
        env.insert(
            format!("VELNOR_FIXTURE_{key}"),
            format!("${{{{ github.{value} }}}}"),
        );
    }
    Ok(velnor_actions_workflow_renderer::shell_step(
        name,
        vec!["bash".to_owned(), "-c".to_owned(), script],
        env,
    )?)
}

#[cfg(test)]
#[path = "workloads_gradle_database_tests.rs"]
mod tests;
