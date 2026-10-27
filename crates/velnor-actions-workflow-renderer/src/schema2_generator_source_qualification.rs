// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{JobTimeout, ReleaseTarget, RoutingWorkflow};

use crate::yaml::Yaml;
use crate::{ProductReleasePins, RenderError, Schema2WorkflowRequest};

pub(super) const WORKFLOW_PATH: &str = ".github/workflows/source-qualification.yml";

const PHASES: &[(&str, &str, bool)] = &[
    (
        "Prepare external evidence directories",
        "prepare-external-evidence-directories",
        true,
    ),
    (
        "Verify exact clean source checkout",
        "verify-exact-clean-source-checkout",
        false,
    ),
    (
        "Read and verify repository tool pins",
        "read-and-verify-repository-tool-pins",
        false,
    ),
    (
        "Install pinned Rust and MBX",
        "install-pinned-rust-and-mbx",
        false,
    ),
    (
        "Record installed tool identities",
        "record-installed-tool-identities",
        false,
    ),
    (
        "Prefetch locked sources for qualification workspaces",
        "prefetch-locked-sources-for-qualification-workspaces",
        false,
    ),
    (
        "Build runner-native archive guard from this checkout",
        "build-runner-native-archive-guard-from-this-checkout",
        false,
    ),
    (
        "Install validator tools with native Mise checksums",
        "install-validator-tools-with-native-mise-checksums",
        false,
    ),
    (
        "Test validator tool lock admission",
        "test-validator-tool-lock-admission",
        false,
    ),
    (
        "Build source CLI with pinned MBX",
        "build-source-cli-with-pinned-mbx",
        false,
    ),
    (
        "Capture all five external goldens",
        "capture-all-five-external-goldens",
        true,
    ),
    (
        "Repeat check against external oracle",
        "repeat-check-against-external-oracle",
        true,
    ),
    (
        "Run full golden shell harness against external oracle",
        "run-full-golden-shell-harness-against-external-oracle",
        true,
    ),
    (
        "Run full golden shell harness with retained workspaces",
        "run-full-golden-shell-harness-with-retained-workspaces",
        true,
    ),
    (
        "Preserve output-binding JSON proof files",
        "preserve-output-binding-json-proof-files",
        true,
    ),
    (
        "Generate twice and retain root-to-preview identities",
        "generate-twice-and-retain-root-to-preview-identities",
        true,
    ),
    (
        "Test generated Rust verifier transport and digest gate",
        "test-generated-rust-verifier-transport-and-digest-gate",
        true,
    ),
    (
        "Check source output against checked-in repository goldens",
        "check-source-output-against-checked-in-repository-goldens",
        true,
    ),
    (
        "Bundle selected qualification evidence",
        "bundle-selected-qualification-evidence",
        true,
    ),
];

pub(super) fn render(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let pins = request.product_release.as_ref().ok_or_else(|| {
        RenderError::InvalidWorkflow("source_qualification_requires_generator_release".to_owned())
    })?;
    if !request
        .workflows
        .contains(&RoutingWorkflow::GeneratorRelease)
    {
        return Err(RenderError::InvalidWorkflow(
            "source_qualification_requires_generator_release".to_owned(),
        ));
    }
    let runner = super::RunnerSpec::hosted(&request.hosted_label)?;
    let mut steps = vec![checkout_step()];
    steps.extend(
        PHASES
            .iter()
            .take(2)
            .map(|(name, phase, always)| phase_step(name, phase, *always)),
    );
    steps.push(mise_step(pins)?);
    steps.extend(
        PHASES
            .iter()
            .skip(2)
            .map(|(name, phase, always)| phase_step(name, phase, *always)),
    );
    steps.push(upload_step());
    let mut job = vec![
        ("name".to_owned(), Yaml::str("Source qualification")),
        ("runs-on".to_owned(), runner.runs_on.clone()),
        (
            "timeout-minutes".to_owned(),
            Yaml::Int(i64::from(
                JobTimeout::new(90)
                    .map_err(|error| RenderError::InvalidWorkflow(error.to_string()))?
                    .minutes(),
            )),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("env".to_owned(), job_env()),
        ("steps".to_owned(), Yaml::Seq(steps)),
    ];
    runner.push_default_shell(&mut job, false);
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Source qualification")),
        (
            "on".to_owned(),
            Yaml::Map(vec![
                ("pull_request".to_owned(), Yaml::Map(Vec::new())),
                ("workflow_dispatch".to_owned(), Yaml::Map(Vec::new())),
            ]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        (
            "concurrency".to_owned(),
            Yaml::Map(vec![
                (
                    "group".to_owned(),
                    Yaml::str("source-qualification-${{ github.ref }}"),
                ),
                ("cancel-in-progress".to_owned(), Yaml::Bool(false)),
            ]),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![("source-qualification".to_owned(), Yaml::Map(job))]),
        ),
    ]))
}

fn job_env() -> Yaml {
    Yaml::Map(vec![
        ("MBX_GC_AUTO".to_owned(), Yaml::str("0")),
        (
            "MISE_BACKENDS_JACTIONLINT".to_owned(),
            Yaml::str("github:jdx/jactionlint"),
        ),
        (
            "MISE_BACKENDS_SHELLCHECK".to_owned(),
            Yaml::str("github:koalaman/shellcheck"),
        ),
        ("MISE_NO_CONFIG".to_owned(), Yaml::str("1")),
        (
            "PR_HEAD_SHA".to_owned(),
            Yaml::str("${{ github.event.pull_request.head.sha || '' }}"),
        ),
        (
            "PR_BASE_SHA".to_owned(),
            Yaml::str("${{ github.event.pull_request.base.sha || '' }}"),
        ),
    ])
}

fn checkout_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Checkout exact event source")),
        ("uses".to_owned(), Yaml::str(crate::steps::CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("ref".to_owned(), Yaml::str("${{ github.sha }}")),
                ("fetch-depth".to_owned(), Yaml::str("2")),
                ("persist-credentials".to_owned(), Yaml::str("false")),
            ]),
        ),
    ])
}

fn mise_step(pins: &ProductReleasePins) -> Result<Yaml, RenderError> {
    let mut step = super::generator_release::mise_setup_step(pins, ReleaseTarget::LinuxX86_64)?;
    let Yaml::Map(fields) = &mut step else {
        return Err(RenderError::InvalidWorkflow(
            "source_qualification_mise_setup_not_map".to_owned(),
        ));
    };
    fields[0].1 = Yaml::str("Set up pinned Mise");
    Ok(step)
}

fn phase_step(name: &str, phase: &str, always: bool) -> Yaml {
    let mut fields = vec![("name".to_owned(), Yaml::str(name))];
    if always {
        fields.push(("if".to_owned(), Yaml::str("always()")));
    }
    fields.extend([
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "run".to_owned(),
            Yaml::str(format!(
                "bash scripts/generator-release/source-qualification.sh {phase}"
            )),
        ),
    ]);
    Yaml::Map(fields)
}

fn upload_step() -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Upload source qualification evidence"),
        ),
        ("if".to_owned(), Yaml::str("always()")),
        (
            "uses".to_owned(),
            Yaml::str(crate::steps::UPLOAD_ARTIFACT_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                (
                    "name".to_owned(),
                    Yaml::str(
                        "source-qualification-${{ github.run_id }}-${{ github.run_attempt }}",
                    ),
                ),
                (
                    "path".to_owned(),
                    Yaml::str("${{ runner.temp }}/source-qualification-artifacts/"),
                ),
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("include-hidden-files".to_owned(), Yaml::Bool(true)),
                ("retention-days".to_owned(), Yaml::Int(14)),
            ]),
        ),
    ])
}
