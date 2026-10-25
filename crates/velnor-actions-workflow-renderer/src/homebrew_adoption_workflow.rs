use crate::{RenderError, render::RenderedFile, schema2::ProductReleasePins, yaml::Yaml};

#[path = "homebrew_adoption_bootstrap.rs"]
mod bootstrap;
#[path = "homebrew_adoption_workflow_jobs.rs"]
mod jobs;

const PATH: &str = ".github/workflows/homebrew-adoption.yml";
const RUNNER: &str = "ubuntu-26.04";
const SDK_REPOSITORY: &str = "tailrocks/velnor-new";
const MANIFEST_SIGNER: &str =
    "tailrocks/velnor-new/.github/workflows/product-release-generator.yml";
const PLAN_ARTIFACT: &str = "velnor-plan-r${{ github.run_id }}-a${{ env.VELNOR_RUN_KEY_ATTEMPT }}";
const RUN_DIR_EXPR: &str =
    "${{ runner.temp }}/velnor/r${{ github.run_id }}-a${{ env.VELNOR_RUN_KEY_ATTEMPT }}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedHomebrewAdoptionCallee {
    pub file: RenderedFile,
}

pub fn render_homebrew_adoption_callee(
    version: &str,
    pins: &ProductReleasePins,
) -> Result<RenderedHomebrewAdoptionCallee, RenderError> {
    crate::marker::validate_version(version)?;
    let document = workflow_document(pins)?;
    let bytes = crate::render::fallback::render_checked_workflow(PATH, &document, version)?;
    let file = RenderedFile {
        path: PATH.to_owned(),
        bytes,
    };
    Ok(RenderedHomebrewAdoptionCallee { file })
}

fn workflow_document(pins: &ProductReleasePins) -> Result<Yaml, RenderError> {
    let inputs = Yaml::Map(vec![
        (
            "transition_id".to_owned(),
            Yaml::Map(vec![
                ("type".to_owned(), Yaml::str("string")),
                ("required".to_owned(), Yaml::Bool(true)),
            ]),
        ),
        (
            "sdk_version".to_owned(),
            Yaml::Map(vec![
                ("type".to_owned(), Yaml::str("string")),
                ("required".to_owned(), Yaml::Bool(true)),
            ]),
        ),
        (
            "sdk_source_sha".to_owned(),
            Yaml::Map(vec![
                ("type".to_owned(), Yaml::str("string")),
                ("required".to_owned(), Yaml::Bool(true)),
            ]),
        ),
        (
            "sdk_manifest_sha256".to_owned(),
            Yaml::Map(vec![
                ("type".to_owned(), Yaml::str("string")),
                ("required".to_owned(), Yaml::Bool(true)),
            ]),
        ),
    ]);
    let common_env = common_identity_env();
    let plan = jobs::plan_job(&common_env, pins)?;
    let native = jobs::native_job(&common_env, pins)?;
    let required = jobs::required_job(&common_env, pins)?;
    let gate = crate::required_gate::required_gate_job_yaml(RUNNER)?;

    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Homebrew adoption")),
        (
            "on".to_owned(),
            Yaml::Map(vec![(
                "workflow_call".to_owned(),
                Yaml::Map(vec![("inputs".to_owned(), inputs)]),
            )]),
        ),
        ("permissions".to_owned(), jobs::readonly_permissions()),
        ("env".to_owned(), common_env),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![plan, native, required, gate]),
        ),
    ]))
}

fn common_identity_env() -> Yaml {
    Yaml::Map(vec![
        (
            "VELNOR_RUN_KEY_ATTEMPT".to_owned(),
            Yaml::str("${{ github.run_attempt }}"),
        ),
        (
            "VELNOR_HOMEBREW_ADOPTION_TRANSITION_ID".to_owned(),
            Yaml::str("${{ inputs.transition_id }}"),
        ),
        (
            "VELNOR_HOMEBREW_ADOPTION_VERSION".to_owned(),
            Yaml::str("${{ inputs.sdk_version }}"),
        ),
        (
            "VELNOR_HOMEBREW_ADOPTION_SOURCE_SHA".to_owned(),
            Yaml::str("${{ inputs.sdk_source_sha }}"),
        ),
        (
            "VELNOR_HOMEBREW_ADOPTION_MANIFEST_SHA256".to_owned(),
            Yaml::str("${{ inputs.sdk_manifest_sha256 }}"),
        ),
    ])
}
