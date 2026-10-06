//! Fresh read-only native qualification jobs; no source builds or signing.

use crate::{Yaml, steps};

use super::{OwnedPublicationSpec, document::map};

pub(super) fn job(spec: &OwnedPublicationSpec, include: Vec<Yaml>) -> Yaml {
    map([
        (
            "name",
            Yaml::str("Qualify / ${{ matrix.tool }} / ${{ matrix.target }}"),
        ),
        ("needs", Yaml::str("build")),
        ("if", Yaml::str("needs.build.result == 'success'")),
        ("runs-on", Yaml::str("${{ matrix.runner }}")),
        ("timeout-minutes", Yaml::Int(20)),
        (
            "permissions",
            map([
                ("contents", Yaml::str("read")),
                ("actions", Yaml::str("read")),
            ]),
        ),
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
                ("OWNED_TOOL_TARGET", Yaml::str("${{ matrix.target }}")),
                ("OWNED_TOOL_SOURCE_JSON", Yaml::str("${{ matrix.source }}")),
                (
                    "OWNED_CANDIDATE_NAME",
                    Yaml::str(
                        "owned-candidate-${{ github.run_id }}-${{ github.run_attempt }}-${{ matrix.tool }}-${{ matrix.target }}",
                    ),
                ),
            ]),
        ),
        ("steps", job_steps(spec)),
    ])
}

fn job_steps(spec: &OwnedPublicationSpec) -> Yaml {
    Yaml::Seq(vec![
        map([
            ("name", Yaml::str("Checkout qualification policy afresh")),
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
            (
                "name",
                Yaml::str("Admit exact immutable candidate artifact"),
            ),
            ("shell", Yaml::str("bash")),
            ("env", map([("GH_TOKEN", Yaml::str("${{ github.token }}"))])),
            (
                "run",
                Yaml::str(
                    "python3 scripts/download-owned-tool-candidate.py --name \"$OWNED_CANDIDATE_NAME\" --output \"$RUNNER_TEMP/velnor-owned-candidate\"",
                ),
            ),
        ]),
        map([
            ("name", Yaml::str("Qualify exact native candidate bytes")),
            ("shell", Yaml::str("bash")),
            (
                "run",
                Yaml::str(
                    "python3 scripts/qualify-owned-tool.py --tool \"$OWNED_TOOL_NAME\" --target \"$OWNED_TOOL_TARGET\" --candidate-directory \"$RUNNER_TEMP/velnor-owned-candidate\" --receipt \"$RUNNER_TEMP/velnor-owned-candidate/qualified-receipt-$OWNED_TOOL_TARGET.json\" --report \"$RUNNER_TEMP/velnor-owned-candidate/native-report-$OWNED_TOOL_TARGET.json\"",
                ),
            ),
        ]),
        map([
            (
                "name",
                Yaml::str("Retain independent qualification evidence once"),
            ),
            ("if", Yaml::str("${{ always() }}")),
            ("uses", Yaml::str(steps::UPLOAD_ARTIFACT_USES)),
            (
                "with",
                map([
                    (
                        "name",
                        Yaml::str(
                            "owned-qualification-${{ github.run_id }}-${{ github.run_attempt }}-${{ matrix.tool }}-${{ matrix.target }}",
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
        ]),
    ])
}
