//! Feature and expected-negative jobs inside qualification.
//! `inputs.mode` selects them. The default `both` keeps the echo lanes.
//! The negative jobs must fail in GitHub. They are not a separate workflow
//! file, because `workflow_dispatch` only sees files on the default branch.
//! One class per dispatch (`js`, `services`, `artifacts`, `buildx`) is one
//! run. `features` still selects every class.

use super::{RunnerSpec, with_if};
use crate::yaml::Yaml;
use velnor_actions_contract::JobTimeout;

const JS: &str = "inputs.mode == 'features' || inputs.mode == 'js'";
const SERVICES: &str = "inputs.mode == 'features' || inputs.mode == 'services'";
const ARTIFACTS: &str = "inputs.mode == 'features' || inputs.mode == 'artifacts'";
const BUILDX: &str = "inputs.mode == 'features' || inputs.mode == 'buildx'";
const NEGATIVE: &str = "inputs.mode == 'negative'";

/// Pinned `actions/checkout` used by qualification jobs.
pub(super) const CHECKOUT_USES: &str = crate::steps::CHECKOUT_USES;
const UPLOAD_USES: &str = crate::steps::UPLOAD_ARTIFACT_USES;
const REDIS_OPTIONS: &str =
    "--health-cmd \"redis-cli ping\" --health-interval 5s --health-timeout 5s --health-retries 12";
const BUILDX_RUN: &str = "docker buildx version && printf 'FROM scratch\\n' > Dockerfile && docker buildx build --progress=plain -t velnor-g4:probe .";

/// Feature jobs. `features` runs every class. A class name runs that class.
pub(super) fn feature_jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    vec![
        gated(
            js_job("js-hosted", "JavaScript actions / GitHub hosted", hosted),
            JS,
        ),
        gated(
            js_job(
                "js-scale-set",
                "JavaScript actions / Velnor Scale Set",
                scale,
            ),
            JS,
        ),
        gated(
            service_job("services-hosted", "Services / GitHub hosted", hosted),
            SERVICES,
        ),
        gated(
            service_job("services-scale-set", "Services / Velnor Scale Set", scale),
            SERVICES,
        ),
        gated(
            artifact_job(
                "artifacts-hosted",
                "Artifacts / GitHub hosted",
                hosted,
                "g4-proof-hosted",
            ),
            ARTIFACTS,
        ),
        gated(
            artifact_job(
                "artifacts-scale-set",
                "Artifacts / Velnor Scale Set",
                scale,
                "g4-proof-scale-set",
            ),
            ARTIFACTS,
        ),
        gated(
            buildx_job("buildx-hosted", "Buildx / GitHub hosted", hosted),
            BUILDX,
        ),
        gated(
            buildx_job("buildx-scale-set", "Buildx / Velnor Scale Set", scale),
            BUILDX,
        ),
    ]
}

/// Jobs that run only when `inputs.mode` is `negative`. They must fail in GitHub.
pub(super) fn negative_jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
    vec![
        gated(
            fail_job(
                "expect-fail-hosted",
                "Expected negative / GitHub hosted",
                hosted,
            ),
            NEGATIVE,
        ),
        gated(
            fail_job(
                "expect-fail-scale-set",
                "Expected negative / Velnor Scale Set",
                scale,
            ),
            NEGATIVE,
        ),
    ]
}

/// Insert `if` at map index 1, matching the qualification document style.
pub(super) fn gated(job: (String, Yaml), when: &str) -> (String, Yaml) {
    with_if(job, when)
}

fn js_job(id: &str, name: &str, runs_on: &RunnerSpec) -> (String, Yaml) {
    finish(
        id,
        lane_base(name, runs_on, JobTimeout::DEFAULT),
        vec![
            checkout_step(),
            run_step("Record checkout", "git rev-parse HEAD"),
            run_step("Run JavaScript", "node -e 'console.log(\"js-action-ok\")'"),
        ],
    )
}

fn service_job(id: &str, name: &str, runs_on: &RunnerSpec) -> (String, Yaml) {
    let mut fields = lane_base(name, runs_on, JobTimeout::DEFAULT);
    fields.push(("services".to_owned(), redis_service()));
    let probe = service_probe(runs_on.service_host(false));
    finish(id, fields, vec![run_step("Service check", &probe)])
}

fn service_probe(host: &str) -> String {
    format!("timeout 20 bash -c 'until echo >/dev/tcp/{host}/6379; do sleep 1; done'")
}

fn artifact_job(id: &str, name: &str, runs_on: &RunnerSpec, artifact: &str) -> (String, Yaml) {
    let mut fields = lane_base(name, runs_on, JobTimeout::DEFAULT);
    fields.push(("permissions".to_owned(), artifact_permissions()));
    finish(
        id,
        fields,
        vec![
            run_step("Write proof", "echo g4-artifact > g4-proof.txt"),
            upload_step(artifact),
        ],
    )
}

fn buildx_job(id: &str, name: &str, runs_on: &RunnerSpec) -> (String, Yaml) {
    finish(
        id,
        lane_base(name, runs_on, JobTimeout::DEFAULT),
        vec![run_step("Buildx probe", BUILDX_RUN)],
    )
}

fn fail_job(id: &str, name: &str, runs_on: &RunnerSpec) -> (String, Yaml) {
    finish(
        id,
        lane_base(name, runs_on, JobTimeout::DEFAULT),
        vec![run_step(
            "Intentional failure",
            "echo expected-negative && exit 1",
        )],
    )
}

/// Shared preamble for typed qualification lanes.
pub(super) fn lane_base(
    name: &str,
    runner: &RunnerSpec,
    timeout: JobTimeout,
) -> Vec<(String, Yaml)> {
    lane_base_with_container(name, runner, timeout, false)
}

/// Shared preamble for a typed lane whose job executes in a container.
pub(super) fn lane_base_with_container(
    name: &str,
    runner: &RunnerSpec,
    timeout: JobTimeout,
    has_container: bool,
) -> Vec<(String, Yaml)> {
    let mut fields = base_fields(name, runner.runs_on.clone(), timeout);
    runner.push_default_shell(&mut fields, has_container);
    fields
}

/// Shared preamble for hosted product-release families.
pub(super) fn base(name: &str, runs_on: Yaml, timeout: JobTimeout) -> Vec<(String, Yaml)> {
    base_fields(name, runs_on, timeout)
}

fn base_fields(name: &str, runs_on: Yaml, timeout: JobTimeout) -> Vec<(String, Yaml)> {
    vec![
        ("name".to_owned(), Yaml::str(name)),
        ("runs-on".to_owned(), runs_on),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(timeout.minutes())),
        ),
    ]
}

/// Append steps and return one job entry.
pub(super) fn finish(
    id: &str,
    mut fields: Vec<(String, Yaml)>,
    steps: Vec<Yaml>,
) -> (String, Yaml) {
    fields.push(("steps".to_owned(), Yaml::Seq(steps)));
    (id.to_owned(), Yaml::Map(fields))
}

/// Redis 7 with the shared health options and published port 6379.
pub(super) fn redis_service() -> Yaml {
    Yaml::Map(vec![(
        "redis".to_owned(),
        Yaml::Map(vec![
            (
                "image".to_owned(),
                Yaml::str(
                    "redis:8.10.2-alpine@sha256:2d3814be5e9b06a30a0be54770b7e12052e7e79ec85271aefd34875c1f393b23",
                ),
            ),
            ("options".to_owned(), Yaml::str(REDIS_OPTIONS)),
            (
                "ports".to_owned(),
                Yaml::Seq(vec![Yaml::quoted("6379:6379")]),
            ),
        ]),
    )])
}

fn artifact_permissions() -> Yaml {
    Yaml::Map(vec![
        ("contents".to_owned(), Yaml::str("read")),
        ("actions".to_owned(), Yaml::str("write")),
    ])
}

/// One `run` step.
pub(super) fn run_step(name: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

/// Release-family publisher carrying a stable role ID through composition.
pub(super) fn identified_publish_step(id: &str, run: &str) -> Yaml {
    let fields = vec![
        ("name".to_owned(), Yaml::str("Publish GitHub release")),
        ("id".to_owned(), Yaml::str(id)),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "GH_TOKEN".to_owned(),
                Yaml::str("${{ github.token }}"),
            )]),
        ),
        ("run".to_owned(), Yaml::str(run)),
    ];
    Yaml::Map(fields)
}

/// Local `./` action, reached after a checkout step. The zizmor
/// `self-repository` ignore stays on this line.
pub(super) fn local_action_step(name: &str, uses: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        (
            "uses".to_owned(),
            Yaml::annotated(uses, "zizmor: ignore[self-repository]"),
        ),
    ])
}

/// Checkout with credentials disabled. Qualification must not persist a token.
pub(super) fn checkout_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out")),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![("persist-credentials".to_owned(), Yaml::str("false"))]),
        ),
    ])
}

fn upload_step(artifact: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Upload proof")),
        ("uses".to_owned(), Yaml::str(UPLOAD_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(artifact)),
                ("path".to_owned(), Yaml::str("g4-proof.txt")),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
            ]),
        ),
    ])
}

#[cfg(test)]
#[path = "schema2_features_tests.rs"]
mod tests;
