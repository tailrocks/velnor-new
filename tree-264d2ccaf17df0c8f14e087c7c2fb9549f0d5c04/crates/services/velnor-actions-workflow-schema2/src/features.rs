//! Feature and expected-negative jobs inside qualification.
//! `inputs.mode` selects them. The default `both` keeps the echo lanes.
//! The negative jobs must fail in GitHub. They are not a separate workflow
//! file, because `workflow_dispatch` only sees files on the default branch.
//! One class per dispatch (`js`, `services`, `artifacts`, `buildx`) is one
//! run. `features` still selects every class.

use super::RunnerSpec;
use super::docker::{docker_endpoint, docker_provider_guard};
use super::workflows::with_if;
use velnor_actions_workflow_tree::job_entries::{CHECKOUT_USES, base, finish};
use velnor_actions_workflow_tree::yaml::Yaml;

const JS: &str = "inputs.mode == 'features' || inputs.mode == 'js'";
const SERVICES: &str = "inputs.mode == 'features' || inputs.mode == 'services'";
const ARTIFACTS: &str = "inputs.mode == 'features' || inputs.mode == 'artifacts'";
const BUILDX: &str = "inputs.mode == 'features' || inputs.mode == 'buildx'";
const NEGATIVE: &str = "inputs.mode == 'negative'";

/// Pinned `actions/checkout` used by qualification jobs.
const UPLOAD_USES: &str = "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a";
const REDIS_OPTIONS: &str =
    "--health-cmd \"redis-cli ping\" --health-interval 5s --health-timeout 5s --health-retries 12";
/// Host-style steps reach a published service port on localhost.
/// The service DNS name is only on the Docker network of a `container:` job.
const PROBE_LOCAL: &str =
    "timeout 20 bash -c 'until echo >/dev/tcp/127.0.0.1/6379; do sleep 1; done'";
const BUILDX_BUILDKIT_IMAGE: &str = "docker.io/moby/buildkit@sha256:98cc6a3fc46220d00f8224ae483f3274fc874e9be8d7dd1e2e2c5481209228b5";
const BUILDX_RUN: &str = r#"set -euo pipefail
endpoint="@@ENDPOINT@@"
builder="velnor-g4-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}"
work="$RUNNER_TEMP/${builder}"
cache="$work/cache"
context="$GITHUB_WORKSPACE/qualification/buildx-context"
case "$work" in "$RUNNER_TEMP"/velnor-g4-*) ;; *) exit 1 ;; esac
mkdir "$work"
test -f "$context/Dockerfile"
test -f "$context/payload.txt"
node "$context/cache-hit.mjs" --self-test
docker --host "$endpoint" buildx create --name "$builder" --driver docker-container --driver-opt "image=@@BUILDKIT_IMAGE@@" "$endpoint" >/dev/null
docker --host "$endpoint" buildx inspect "$builder" --bootstrap > "$work/inspect.txt"
awk -v endpoint="$endpoint" '$1 == "Driver:" && $2 == "docker-container" { driver=1 } $1 == "Endpoint:" && $2 == endpoint { route=1 } $1 == "BuildKit:" && $2 == "v0.33.1" { version=1 } END { exit !(driver && route && version) }' "$work/inspect.txt"
if ! docker --host "$endpoint" buildx build --builder "$builder" --platform linux/amd64 --progress=plain --cache-to "type=local,dest=$cache" --output "type=local,dest=$work/first-result" --file "$context/Dockerfile" "$context" > "$work/first-build.log" 2>&1; then
  cat "$work/first-build.log"
  exit 1
fi
test -s "$cache/index.json"
cmp "$context/payload.txt" "$work/first-result/payload.txt"
docker --host "$endpoint" buildx prune --builder "$builder" --all --force
if ! docker --host "$endpoint" buildx build --builder "$builder" --platform linux/amd64 --progress=plain --cache-from "type=local,src=$cache" --output "type=local,dest=$work/second-result" --file "$context/Dockerfile" "$context" > "$work/second-build.log" 2>&1; then
  cat "$work/second-build.log"
  exit 1
fi
if ! node "$context/cache-hit.mjs" "$work/second-build.log"; then
  cat "$work/second-build.log"
  printf '%s\n' 'Buildx did not report a hit for the pinned local context' >&2
  exit 1
fi
cmp "$context/payload.txt" "$work/second-result/payload.txt""#;
const BUILDX_CLEANUP: &str = r#"set -euo pipefail
endpoint="@@ENDPOINT@@"
builder="velnor-g4-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}"
work="$RUNNER_TEMP/${builder}"
case "$work" in "$RUNNER_TEMP"/velnor-g4-*) ;; *) exit 1 ;; esac
mkdir -p "$work"
docker --host "$endpoint" buildx ls --format '{{.Name}}' > "$work/builders-before-cleanup"
if grep -Fxq "$builder" "$work/builders-before-cleanup"; then
  docker --host "$endpoint" buildx rm "$builder"
fi
docker --host "$endpoint" buildx ls --format '{{.Name}}' > "$work/builders-after-cleanup"
if grep -Fxq "$builder" "$work/builders-after-cleanup"; then
  printf '%s\n' 'Buildx builder remained after cleanup' >&2
  exit 1
fi
rm -rf "$work""#;

/// Feature jobs. `features` runs every class. A class name runs that class.
pub(crate) fn feature_jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
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
pub(crate) fn negative_jobs(hosted: &RunnerSpec, scale: &RunnerSpec) -> Vec<(String, Yaml)> {
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
pub(crate) fn gated(job: (String, Yaml), when: &str) -> (String, Yaml) {
    with_if(job, when)
}

fn js_job(id: &str, name: &str, runs_on: &RunnerSpec) -> (String, Yaml) {
    finish(
        id,
        lane_base(name, runs_on, 20),
        vec![
            checkout_step(),
            run_step("Record checkout", "git rev-parse HEAD"),
            run_step("Run JavaScript", "node -e 'console.log(\"js-action-ok\")'"),
        ],
    )
}

fn service_job(id: &str, name: &str, runs_on: &RunnerSpec) -> (String, Yaml) {
    let mut fields = lane_base(name, runs_on, 20);
    fields.push(("services".to_owned(), redis_service()));
    finish(id, fields, vec![run_step("Localhost port", PROBE_LOCAL)])
}

fn artifact_job(id: &str, name: &str, runs_on: &RunnerSpec, artifact: &str) -> (String, Yaml) {
    let mut fields = lane_base(name, runs_on, 20);
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
    let (guard_name, guard_script) = docker_provider_guard(runs_on);
    let endpoint = docker_endpoint(runs_on);
    let probe = BUILDX_RUN
        .replace("@@ENDPOINT@@", endpoint)
        .replace("@@BUILDKIT_IMAGE@@", BUILDX_BUILDKIT_IMAGE);
    let cleanup = BUILDX_CLEANUP.replace("@@ENDPOINT@@", endpoint);
    finish(
        id,
        lane_base(name, runs_on, 20),
        vec![
            run_step(guard_name, guard_script),
            checkout_step(),
            run_step("Buildx pinned context/cache probe", &probe),
            always_run_step("Remove Buildx builder and local cache", &cleanup),
        ],
    )
}

fn always_run_step(name: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("if".to_owned(), Yaml::str("always()")),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

fn fail_job(id: &str, name: &str, runs_on: &RunnerSpec) -> (String, Yaml) {
    finish(
        id,
        lane_base(name, runs_on, 10),
        vec![run_step(
            "Intentional failure",
            "echo expected-negative && exit 1",
        )],
    )
}

/// Shared preamble for typed qualification lanes.
pub(crate) fn lane_base(name: &str, runner: &RunnerSpec, timeout: i64) -> Vec<(String, Yaml)> {
    lane_base_with_container(name, runner, timeout, false)
}

/// Shared preamble for a typed lane whose job executes in a container.
pub(crate) fn lane_base_with_container(
    name: &str,
    runner: &RunnerSpec,
    timeout: i64,
    has_container: bool,
) -> Vec<(String, Yaml)> {
    let mut fields = base(name, runner.runs_on.clone(), timeout);
    runner.push_default_shell(&mut fields, has_container);
    fields
}

/// Shared preamble for hosted product-release families.
/// Redis 7 with the shared health options and published port 6379.
pub(crate) fn redis_service() -> Yaml {
    Yaml::Map(vec![(
        "redis".to_owned(),
        Yaml::Map(vec![
            ("image".to_owned(), Yaml::str("redis:7-alpine")),
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
pub(crate) fn run_step(name: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

/// `gh` reads `GH_TOKEN`. The job token is not a dispatch input.
pub(crate) fn publish_step(run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Publish GitHub release")),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "GH_TOKEN".to_owned(),
                Yaml::str("${{ github.token }}"),
            )]),
        ),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

/// Local `./` action. actionlint 1.7.12 rejects `$/`, so the
/// self-repository auto-fix cannot be applied. The ignore stays on this line.
pub(crate) fn local_action_step(name: &str, uses: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        (
            "uses".to_owned(),
            Yaml::annotated(uses, "zizmor: ignore[self-repository]"),
        ),
    ])
}

/// Checkout with credentials disabled. Qualification must not persist a token.
pub(crate) fn checkout_step() -> Yaml {
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
