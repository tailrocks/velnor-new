//! Versioned generator release with build attestations and immutable publish.

use crate::RenderError;
use crate::runs_on::runs_on_yaml;
use crate::yaml::Yaml;

use super::Schema2WorkflowRequest;

#[path = "schema2_generator_release_assets.rs"]
mod assets;
#[path = "schema2_generator_release_support.rs"]
mod support;

/// GitHub-hosted macOS ARM64 builder.
const MACOS_RUNS_ON: &str = "macos-15";

/// Render a source-bound two-target generator release workflow.
///
/// # Errors
///
/// Illegal hosted or macOS labels fail.
pub(super) fn generator_release(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let hosted = runs_on_yaml("ubuntu-22.04")?;
    let macos = runs_on_yaml(MACOS_RUNS_ON)?;
    let assets = assets::ReleaseAssets::new(&request.version)?;
    let jobs = vec![
        support::eligibility_job(hosted.clone()),
        support::linux_build_job(hosted.clone(), &assets),
        support::linux_attest_job(hosted.clone(), &assets),
        support::macos_build_job(macos.clone(), &assets),
        support::macos_attest_job(macos, &assets),
        support::manifest_job(hosted.clone(), &assets),
        support::publish_job(hosted, &assets),
    ];
    Ok(document(jobs))
}

fn document(jobs: Vec<(String, Yaml)>) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Generator release")),
        (
            "on".to_owned(),
            Yaml::Map(vec![("workflow_dispatch".to_owned(), Yaml::Map(vec![]))]),
        ),
        (
            "permissions".to_owned(),
            Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))]),
        ),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ])
}

#[cfg(test)]
#[path = "schema2_generator_release_tests.rs"]
mod tests;
