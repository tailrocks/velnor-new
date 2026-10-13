//! Candidate execution on an isolated, read-only qualification runner.

use crate::RenderError;
use crate::yaml::Yaml;

use super::super::features::{base, finish};
use super::assets::{self, ProductAsset};
use super::workflow_steps::{self, checkout_step, with_permissions};

/// Qualify a build artifact in a job that has no write or attestation permissions.
pub(super) fn job(
    id: &str,
    name: &str,
    action: &str,
    runs_on: Yaml,
    build_job: &str,
    product: ProductAsset,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<(String, Yaml), RenderError> {
    let mut action_steps = vec![
        workflow_steps::mise_step(),
        workflow_steps::bash_step(
            "Install pinned candidate qualification tools",
            assets::QUALIFICATION_TOOLS_INSTALL,
        ),
    ];
    action_steps.extend(assets::download_build_steps(
        product,
        "Download built asset archive",
        build_job,
    ));
    action_steps.extend([
        workflow_steps::bash_step(
            "Verify candidate provenance record",
            &assets::verify_provenance_script(product),
        ),
        workflow_steps::bash_step(
            "Verify downloaded checksum sidecar",
            &format!(
                "set -eu\ncd {}\n{} {}",
                product.directory, product.checksum_command, product.sidecar
            ),
        ),
        workflow_steps::bash_step(
            "Qualify downloaded candidate",
            &assets::qualification_script(product.binary, product.directory),
        ),
    ]);
    let call = super::jobs::local_action(action, name, action_steps, actions)?;
    Ok(finish(
        id,
        with_permissions(
            workflow_steps::with_needs(base(name, runs_on, 120), &[build_job]),
            workflow_steps::qualification_permissions(),
        ),
        vec![checkout_step(), call],
    ))
}
