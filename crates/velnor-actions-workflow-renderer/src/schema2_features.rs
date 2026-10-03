//! Extra qualification workflows. Echo qualification stays a separate file.
//! The negative workflow is expected to fail in GitHub.

use super::{document, empty_dispatch};
use crate::runs_on::runs_on_yaml;
use crate::yaml::Yaml;
use crate::{RenderError, Schema2WorkflowRequest};

const CHECKOUT_USES: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const UPLOAD_USES: &str = "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a";
const REDIS_OPTIONS: &str =
    "--health-cmd \"redis-cli ping\" --health-interval 5s --health-timeout 5s --health-retries 12";
const PROBE_DNS: &str = "node -e 'const n=require(\"net\");const s=n.connect(6379,\"redis\",()=>s.end());s.on(\"error\",()=>process.exit(1));setTimeout(()=>process.exit(1),10000)'";
const PROBE_LOCAL: &str = "node -e 'const n=require(\"net\");const s=n.connect(6379,\"127.0.0.1\",()=>s.end());s.on(\"error\",()=>process.exit(1));setTimeout(()=>process.exit(1),10000)'";
const BUILDX_RUN: &str = "docker buildx version && printf 'FROM scratch\\n' > Dockerfile && docker buildx build --progress=plain -t velnor-g4:probe .";

/// JavaScript, service, artifact, and Buildx jobs on both lanes.
///
/// # Errors
///
/// Illegal selectors fail.
pub(super) fn features(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let hosted = runs_on_yaml(&request.hosted_label)?;
    let scale = runs_on_yaml(&request.scale_set.token())?;
    Ok(document(
        "Qualification features",
        empty_dispatch(),
        vec![
            js_job(
                "js-hosted",
                "JavaScript actions / GitHub hosted",
                hosted.clone(),
            ),
            js_job(
                "js-scale-set",
                "JavaScript actions / Velnor Scale Set",
                scale.clone(),
            ),
            service_job(
                "services-hosted",
                "Services / GitHub hosted",
                hosted.clone(),
            ),
            service_job(
                "services-scale-set",
                "Services / Velnor Scale Set",
                scale.clone(),
            ),
            artifact_job(
                "artifacts-hosted",
                "Artifacts / GitHub hosted",
                hosted.clone(),
                "g4-proof-hosted",
            ),
            artifact_job(
                "artifacts-scale-set",
                "Artifacts / Velnor Scale Set",
                scale.clone(),
                "g4-proof-scale-set",
            ),
            buildx_job("buildx-hosted", "Buildx / GitHub hosted", hosted),
            buildx_job("buildx-scale-set", "Buildx / Velnor Scale Set", scale),
        ],
    ))
}

/// Hosted and scale-set jobs that must fail in GitHub.
///
/// # Errors
///
/// Illegal selectors fail.
pub(super) fn negative(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let hosted = runs_on_yaml(&request.hosted_label)?;
    let scale = runs_on_yaml(&request.scale_set.token())?;
    Ok(document(
        "Qualification negative",
        empty_dispatch(),
        vec![
            fail_job(
                "expect-fail-hosted",
                "Expected negative / GitHub hosted",
                hosted,
            ),
            fail_job(
                "expect-fail-scale-set",
                "Expected negative / Velnor Scale Set",
                scale,
            ),
        ],
    ))
}

fn js_job(id: &str, name: &str, runs_on: Yaml) -> (String, Yaml) {
    finish(
        id,
        base(name, runs_on, 20),
        vec![
            uses_step("Check out", CHECKOUT_USES),
            run_step("Record checkout", "git rev-parse HEAD"),
            run_step("Run JavaScript", "node -e 'console.log(\"js-action-ok\")'"),
        ],
    )
}

fn service_job(id: &str, name: &str, runs_on: Yaml) -> (String, Yaml) {
    let mut fields = base(name, runs_on, 20);
    fields.push(("services".to_owned(), redis_service()));
    finish(
        id,
        fields,
        vec![
            run_step("Service DNS", PROBE_DNS),
            run_step("Localhost port", PROBE_LOCAL),
        ],
    )
}

fn artifact_job(id: &str, name: &str, runs_on: Yaml, artifact: &str) -> (String, Yaml) {
    let mut fields = base(name, runs_on, 20);
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

fn buildx_job(id: &str, name: &str, runs_on: Yaml) -> (String, Yaml) {
    finish(
        id,
        base(name, runs_on, 20),
        vec![run_step("Buildx probe", BUILDX_RUN)],
    )
}

fn fail_job(id: &str, name: &str, runs_on: Yaml) -> (String, Yaml) {
    finish(
        id,
        base(name, runs_on, 10),
        vec![run_step(
            "Intentional failure",
            "echo expected-negative && exit 1",
        )],
    )
}

fn base(name: &str, runs_on: Yaml, timeout: i64) -> Vec<(String, Yaml)> {
    vec![
        ("name".to_owned(), Yaml::str(name)),
        ("runs-on".to_owned(), runs_on),
        ("timeout-minutes".to_owned(), Yaml::Int(timeout)),
    ]
}

fn finish(id: &str, mut fields: Vec<(String, Yaml)>, steps: Vec<Yaml>) -> (String, Yaml) {
    fields.push(("steps".to_owned(), Yaml::Seq(steps)));
    (id.to_owned(), Yaml::Map(fields))
}

fn redis_service() -> Yaml {
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

fn run_step(name: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

fn uses_step(name: &str, uses: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(uses)),
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
