//! Generator-only source approval and source candidate workflow assembly.

use std::path::Path;

use velnor_actions_contract::{WorkflowPolicy, parse_strict_json};
use velnor_actions_mise::catalog::owned_source::ApprovedOwnedSource;
use velnor_actions_mise::catalog::source_build_bootstrap::{
    SourceBuildBootstrapAsset, SourceBuildBootstrapFormat, SourceBuildBootstrapHost,
    SourceBuildBootstrapTool, official,
};
use velnor_actions_workflow_renderer::RenderedFile;
use velnor_actions_workflow_renderer::owned_tool_publication::{
    OwnedBuildBootstrap, OwnedPublicationSpec, SourceBuildBinding, SourceQualificationTrigger,
    render_owned_publication_files,
};

use crate::{OrchestratorError, prepare::GenerationPreparation, workflow::CHECKOUT_USES};

/// Reviewed generator-only staging authority. Consumers do not read this file.
pub(crate) const SOURCE_APPROVAL_PATH: &str = ".velnor/owned-tool-sources.json";

pub(crate) fn files(prep: &GenerationPreparation) -> Result<Vec<RenderedFile>, OrchestratorError> {
    if prep.config.workflow.policy != WorkflowPolicy::VelnorRepositoryV1 {
        return Ok(Vec::new());
    }
    files_for_root(&prep.root)
}

pub(crate) fn files_for_root(root: &Path) -> Result<Vec<RenderedFile>, OrchestratorError> {
    files_for_trigger(root, SourceQualificationTrigger::DefaultBranchDispatch)
}

pub(crate) fn files_for_trigger(
    root: &Path,
    trigger: SourceQualificationTrigger,
) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let sources = sources(root)?;
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    Ok(render_owned_publication_files(&OwnedPublicationSpec {
        trigger,
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        sources,
        bootstraps: bootstraps()?,
    })?)
}

fn sources(root: &Path) -> Result<Vec<SourceBuildBinding>, OrchestratorError> {
    let text = match crate::safe_read::read_repo_file(root, SOURCE_APPROVAL_PATH, 65536)? {
        crate::safe_read::RepoRead::Absent => return Ok(Vec::new()),
        crate::safe_read::RepoRead::Text(text) => text,
    };
    let value = parse_strict_json(&text)?;
    let sources: Vec<ApprovedOwnedSource> =
        serde_json::from_value(value).map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?;
    sources
        .into_iter()
        .map(|source| {
            source
                .validate()
                .map_err(|error| OrchestratorError::Contract {
                    problem: error.to_string(),
                })?;
            let source_json =
                serde_json::to_string(&source).map_err(|error| OrchestratorError::Contract {
                    problem: error.to_string(),
                })?;
            Ok(SourceBuildBinding {
                label: source.tool,
                source_json,
            })
        })
        .collect()
}

fn bootstraps() -> Result<Vec<OwnedBuildBootstrap>, OrchestratorError> {
    [
        (
            SourceBuildBootstrapHost::LinuxAmd64,
            "ubuntu-26.04",
            "x86_64-unknown-linux-gnu",
        ),
        (
            SourceBuildBootstrapHost::LinuxArm64,
            "ubuntu-24.04-arm",
            "aarch64-unknown-linux-gnu",
        ),
        (
            SourceBuildBootstrapHost::MacosArm64,
            "macos-26",
            "aarch64-apple-darwin",
        ),
    ]
    .into_iter()
    .map(|(host, runner, target)| {
        let assets = [
            ("mise", SourceBuildBootstrapTool::Mise),
            ("mbx", SourceBuildBootstrapTool::Mbx),
        ]
        .into_iter()
        .map(|(name, tool)| (name.to_owned(), asset_json(official(tool, host))))
        .collect::<serde_json::Map<_, _>>();
        (assets, runner, target)
    })
    .map(|(assets, runner, target)| {
        Ok(OwnedBuildBootstrap {
            target: target.to_owned(),
            runner: runner.to_owned(),
            assets_json: serde_json::to_string(&assets).map_err(|error| {
                OrchestratorError::Contract {
                    problem: error.to_string(),
                }
            })?,
        })
    })
    .collect()
}

fn asset_json(asset: SourceBuildBootstrapAsset) -> serde_json::Value {
    serde_json::json!({
        "url": asset.asset_url(),
        "archive_sha256": asset.archive_sha256(),
        "binary_sha256": asset.binary_sha256(),
        "format": match asset.asset_format() {
            SourceBuildBootstrapFormat::Binary => "standalone",
            SourceBuildBootstrapFormat::TarGzip => "tar.gz",
        },
        "binary_member": asset.binary_member(),
        "version": asset.version(),
        "source_repository": asset.source_repository(),
        "source_commit": asset.source_commit(),
        "source_tree": asset.source_tree(),
    })
}
