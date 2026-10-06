//! Mechanical rendering of closed owned-source qualification event categories.

use crate::{Yaml, steps};

use super::{OwnedPublicationSpec, REVIEWED_INFRASTRUCTURE_BRANCH, SourceQualificationTrigger};

pub(super) fn map(entries: impl IntoIterator<Item = (&'static str, Yaml)>) -> Yaml {
    Yaml::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

pub(super) fn document(spec: &OwnedPublicationSpec) -> Yaml {
    let mut include = Vec::new();
    for source in &spec.sources {
        let source_json = &source.source_json;
        for bootstrap in &spec.bootstraps {
            include.push(map([
                ("tool", Yaml::str(&source.label)),
                ("target", Yaml::str(&bootstrap.target)),
                ("runner", Yaml::str(&bootstrap.runner)),
                ("source", Yaml::str(source_json)),
                ("bootstrap_assets", Yaml::str(&bootstrap.assets_json)),
            ]));
        }
    }
    map([
        ("name", Yaml::str("Owned tool candidates")),
        ("on", trigger(spec.trigger)),
        ("permissions", map([])),
        (
            "concurrency",
            map([
                ("group", Yaml::str("owned-tool-candidates")),
                ("cancel-in-progress", Yaml::Bool(false)),
            ]),
        ),
        (
            "jobs",
            map([
                ("build", build_job(spec, include.clone())),
                ("qualify", super::qualification::job(spec, include)),
            ]),
        ),
    ])
}

fn build_job(spec: &OwnedPublicationSpec, include: Vec<Yaml>) -> Yaml {
    map([
        (
            "name",
            Yaml::str("Candidate / ${{ matrix.tool }} / ${{ matrix.target }}"),
        ),
        ("if", Yaml::str(job_guard(spec.trigger))),
        ("runs-on", Yaml::str("${{ matrix.runner }}")),
        ("timeout-minutes", Yaml::Int(90)),
        ("permissions", map([("contents", Yaml::str("read"))])),
        (
            "strategy",
            map([
                ("fail-fast", Yaml::Bool(false)),
                ("max-parallel", Yaml::Int(3)),
                ("matrix", map([("include", Yaml::Seq(include))])),
            ]),
        ),
        (
            "env",
            map([
                ("OWNED_TOOL_NAME", Yaml::str("${{ matrix.tool }}")),
                ("OWNED_TOOL_SOURCE_JSON", Yaml::str("${{ matrix.source }}")),
                ("OWNED_TOOL_TARGET", Yaml::str("${{ matrix.target }}")),
                (
                    "OWNED_TOOL_BUILD_BOOTSTRAP_JSON",
                    Yaml::str("${{ matrix.bootstrap_assets }}"),
                ),
            ]),
        ),
        ("steps", build_steps(spec)),
    ])
}

fn trigger(trigger: SourceQualificationTrigger) -> Yaml {
    match trigger {
        SourceQualificationTrigger::DefaultBranchDispatch => map([("workflow_dispatch", map([]))]),
        SourceQualificationTrigger::ReviewedInfrastructurePush => map([(
            "push",
            map([(
                "branches",
                Yaml::Seq(vec![Yaml::str(REVIEWED_INFRASTRUCTURE_BRANCH)]),
            )]),
        )]),
    }
}

fn job_guard(trigger: SourceQualificationTrigger) -> String {
    let event = match trigger {
        SourceQualificationTrigger::DefaultBranchDispatch => "github.event_name == 'workflow_dispatch' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch)".to_owned(),
        SourceQualificationTrigger::ReviewedInfrastructurePush => format!(
            "github.event_name == 'push' && github.ref == 'refs/heads/{REVIEWED_INFRASTRUCTURE_BRANCH}' && github.event.repository.default_branch != '{REVIEWED_INFRASTRUCTURE_BRANCH}'"
        ),
    };
    format!(
        "github.repository == 'tailrocks/velnor-new' && {event} && github.workflow_ref == format('tailrocks/velnor-new/.github/workflows/owned-tools.yml@{{0}}', github.ref) && github.workflow_sha == github.sha"
    )
}

fn build_steps(spec: &OwnedPublicationSpec) -> Yaml {
    Yaml::Seq(vec![
        map([
            ("name", Yaml::str("Checkout generator policy")),
            ("uses", Yaml::str(&spec.checkout_uses)),
            (
                "with",
                map([
                    ("ref", Yaml::str("${{ github.sha }}")),
                    ("persist-credentials", Yaml::Bool(false)),
                    ("fetch-depth", Yaml::Int(1)),
                ]),
            ),
        ]),
        map([
            ("name", Yaml::str("Acquire verified build bootstrap")),
            ("shell", Yaml::str("bash")),
            (
                "run",
                Yaml::str("python3 scripts/bootstrap-owned-tool-builder.py"),
            ),
        ]),
        map([
            ("name", Yaml::str("Build approved source candidate")),
            ("shell", Yaml::str("bash")),
            (
                "run",
                Yaml::str(
                    "python3 scripts/build-owned-tool.py --output \"$RUNNER_TEMP/velnor-owned-candidate\"",
                ),
            ),
        ]),
        upload_step(),
    ])
}

fn upload_step() -> Yaml {
    map([
        ("name", Yaml::str("Upload exact source candidate once")),
        ("if", Yaml::str("${{ always() }}")),
        ("id", Yaml::str("candidate")),
        ("uses", Yaml::str(steps::UPLOAD_ARTIFACT_USES)),
        (
            "with",
            map([
                (
                    "name",
                    Yaml::str(
                        "owned-candidate-${{ github.run_id }}-${{ github.run_attempt }}-${{ matrix.tool }}-${{ matrix.target }}",
                    ),
                ),
                (
                    "path",
                    Yaml::str("${{ runner.temp }}/velnor-owned-candidate"),
                ),
                ("if-no-files-found", Yaml::str("error")),
                ("retention-days", Yaml::Int(7)),
                ("overwrite", Yaml::Bool(false)),
            ]),
        ),
    ])
}
