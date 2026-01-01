//! Per-product asset contracts and immutable release scripts.

use std::collections::BTreeMap;

use crate::RenderError;
use crate::schema2::ProductReleasePins;
use velnor_actions_contract::ReleaseTarget;

use super::release_eligibility as eligibility;

const PREPARE_TEMPLATE: &str = include_str!("schema2_release_prepare.sh");
const PUBLISH_TEMPLATE: &str = include_str!("schema2_release_publish.sh");

/// Stable ID for the legacy family publisher before typed composition.
pub(super) const PUBLISH_STEP_ID: &str = "publish-family-release";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StepRole {
    Publish,
}

pub(super) fn step_role(id: &str) -> Option<StepRole> {
    (id == PUBLISH_STEP_ID).then_some(StepRole::Publish)
}

/// One independently published family inside the composed release workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    /// Linux runner and `DinD` image archives.
    Images,
    /// macOS Velnor host binary.
    Binary,
    /// Linux and macOS Velnor generator binaries.
    Generator,
}

impl Family {
    /// Stable release tag prefix retained for existing consumers.
    pub(super) const fn tag_prefix(self) -> &'static str {
        match self {
            Self::Images => "runner",
            Self::Binary => "binary",
            Self::Generator => "",
        }
    }

    fn fixed_tag(self) -> Option<String> {
        match self {
            Self::Images | Self::Binary => None,
            Self::Generator => Some(format!("v{}", super::generator_release::release_version())),
        }
    }

    /// Build and attestation IDs for the two legacy non-generator families.
    pub(super) const fn job_ids(
        self,
    ) -> Option<(
        &'static [&'static str],
        &'static [&'static str],
        &'static str,
    )> {
        match self {
            Self::Images => Some((&["build-images"], &["attest-images"], "publish-images")),
            Self::Binary => Some((&["build-binary"], &["attest-binary"], "publish-binary")),
            Self::Generator => None,
        }
    }

    /// Stable preparation job identity.
    pub(super) const fn prepare_id(self) -> &'static str {
        match self {
            Self::Images => "prepare-images",
            Self::Binary => "prepare-binary",
            Self::Generator => "prepare-generator",
        }
    }

    /// Reusable workflow path reached by the dispatch-only coordinator.
    pub(super) const fn workflow_path(self) -> &'static str {
        match self {
            Self::Images => ".github/workflows/product-release-images.yml",
            Self::Binary => ".github/workflows/product-release-binary.yml",
            Self::Generator => ".github/workflows/product-release-generator.yml",
        }
    }

    /// Stable caller job identity for this reusable workflow.
    pub(super) const fn call_id(self) -> &'static str {
        match self {
            Self::Images => "release-images",
            Self::Binary => "release-binary",
            Self::Generator => "release-generator",
        }
    }

    /// Runner target that owns this family's pinned GitHub CLI setup.
    pub(super) const fn runner_target(self) -> ReleaseTarget {
        match self {
            Self::Images | Self::Generator => ReleaseTarget::LinuxX86_64,
            Self::Binary => ReleaseTarget::MacosArm64,
        }
    }

    /// Human-readable family label for job names and release notes.
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Images => "runner images",
            Self::Binary => "velnor-host binary",
            Self::Generator => "velnor-actions generator",
        }
    }

    /// Paths where same-run workflow artifacts are downloaded.
    fn asset_paths(self) -> Vec<String> {
        match self {
            Self::Images => [
                "velnor-runner-linux-amd64.tar",
                "velnor-dind-linux-amd64.tar",
                "SHA256SUMS",
            ]
            .map(|name| format!("assets/{name}"))
            .to_vec(),
            Self::Binary => ["velnor-host", "SHA256SUMS"]
                .map(|name| format!("assets/{name}"))
                .to_vec(),
            Self::Generator => super::generator_release::publication_asset_paths(),
        }
    }

    fn asset_names(self) -> Vec<String> {
        self.asset_paths()
            .into_iter()
            .filter_map(|path| path.rsplit('/').next().map(str::to_owned))
            .collect()
    }
}

/// Install Mise and the one canonical GitHub CLI version.
/// Validate an existing release or require the exact tag to be absent.
pub(super) fn prepare_script(
    family: Family,
    pins: &ProductReleasePins,
) -> Result<String, RenderError> {
    let checksum = checksum_check(family, "$temp_dir", Some(pins))?;
    let downloads = download_commands(family)?;
    let gh_function = super::generator_release::gh_function(pins)?;
    Ok(PREPARE_TEMPLATE
        .replace("@GH_FUNCTION@", &gh_function)
        .replace("@REPOSITORY@", eligibility::REPOSITORY)
        .replace("@TAG_PREFIX@", family.tag_prefix())
        .replace("@FIXED_TAG@", family.fixed_tag().as_deref().unwrap_or(""))
        .replace("@LABEL@", family.label())
        .replace(
            "@ASSET_NAMES_JSON@",
            &json_asset_array(&family.asset_names()),
        )
        .replace("@DOWNLOAD_COMMANDS@", &downloads)
        .replace("@CHECKSUM_COMMAND@", &checksum)
        .replace(
            "@ASSET_VERIFY_COMMANDS@",
            &prepare_asset_verification(family),
        ))
}

/// Recheck source eligibility, then publish or idempotently validate one family.
pub(super) fn publish_script(
    family: Family,
    pins: &ProductReleasePins,
) -> Result<String, RenderError> {
    if family == Family::Generator {
        return Err(RenderError::InvalidWorkflow(
            "generator_publisher_must_remain_inside_typed_graph".to_owned(),
        ));
    }
    let gh_function = super::generator_release::gh_function(pins)?;
    Ok(PUBLISH_TEMPLATE
        .replace("@GH_FUNCTION@", &gh_function)
        .replace(
            "@ELIGIBILITY_SCRIPT@",
            &eligibility::publisher_script(pins)?,
        )
        .replace("@PREPARE_SCRIPT@", &prepare_script(family, pins)?)
        .replace("@REPOSITORY@", eligibility::REPOSITORY)
        .replace("@WORKFLOW_PATH@", family.workflow_path())
        .replace("@TAG_PREFIX@", family.tag_prefix())
        .replace("@FIXED_TAG@", family.fixed_tag().as_deref().unwrap_or(""))
        .replace("@LABEL@", family.label())
        .replace(
            "@ASSET_NAMES_JSON@",
            &json_asset_array(&family.asset_names()),
        )
        .replace("@ASSET_PATHS@", &shell_words(&family.asset_paths()))
        .replace(
            "@CHECKSUM_COMMAND@",
            &checksum_check(family, ".", Some(pins))?,
        )
        .replace(
            "@ASSET_VERIFY_COMMANDS@",
            &publish_asset_verification(family),
        ))
}

/// Return commands that download each asset into its consumer-compatible path.
fn download_commands(family: Family) -> Result<String, RenderError> {
    match family {
        Family::Images | Family::Binary => Ok(format!(
            "mkdir -p \"$temp_dir/assets\" || fail 'could not create release asset directory'\ngh release download \"$prepare_tag\" --repo \"$repository\" --dir \"$temp_dir/assets\" {} || fail 'release asset download failed'",
            download_patterns(&family.asset_names())
        )),
        Family::Generator => grouped_download_commands(family.asset_paths()),
    }
}

fn grouped_download_commands(paths: Vec<String>) -> Result<String, RenderError> {
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for path in paths {
        let Some((directory, name)) = path.rsplit_once('/') else {
            return Err(RenderError::InvalidWorkflow(
                "generator_release_asset_path_missing_directory".to_owned(),
            ));
        };
        groups
            .entry(directory.to_owned())
            .or_default()
            .push(name.to_owned());
    }
    if groups.is_empty() {
        return Err(RenderError::InvalidWorkflow(
            "generator_release_asset_inventory_empty".to_owned(),
        ));
    }
    Ok(groups
        .into_iter()
        .map(|(directory, assets)| {
            format!(
                "mkdir -p \"$temp_dir/{directory}\" || fail 'could not create release asset directory: {directory}'\ngh release download \"$prepare_tag\" --repo \"$repository\" --dir \"$temp_dir/{directory}\" {} || fail 'release asset download failed: {directory}'",
                download_patterns(&assets)
            )
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

/// Return local checksum validation commands for downloaded or new assets.
fn checksum_check(
    family: Family,
    root: &str,
    pins: Option<&ProductReleasePins>,
) -> Result<String, RenderError> {
    match family {
        Family::Images | Family::Binary => Ok(format!(
            "(cd \"{root}/assets\" && shasum -a 256 --check SHA256SUMS) || fail 'release asset checksum verification failed'"
        )),
        Family::Generator => {
            let pins = pins.ok_or_else(|| {
                RenderError::InvalidWorkflow("product_release_pins_missing".to_owned())
            })?;
            Ok(super::generator_release::verify_published_manifest_script(
                pins,
            ))
        }
    }
}

fn prepare_asset_verification(family: Family) -> String {
    let assets = family.asset_paths();
    let mut commands = assets
        .iter()
        .map(|path| {
            if family == Family::Generator {
                format!(
                    "gh release verify-asset \"$prepare_tag\" \"$temp_dir/{path}\" --repo \"$repository\" >/dev/null || fail 'release asset verification failed: {path}'"
                )
            } else {
                format!(
                    "gh release verify-asset \"$prepare_tag\" \"$temp_dir/{path}\" --repo \"$repository\" >/dev/null || fail 'release asset verification failed: {path}'\ngh attestation verify \"$temp_dir/{path}\" --repo \"$repository\" --source-ref refs/heads/main --source-digest \"$source_sha\" --signer-workflow \"$repository/{}\" --signer-digest \"$authority_sha\" >/dev/null || fail 'asset attestation verification failed: {path}'",
                    family.workflow_path()
                )
            }
        })
        .collect::<Vec<_>>();
    if family == Family::Generator {
        commands.push(format!(
            "(cd \"$temp_dir\" && {}) || fail 'generator attestation bundle verification failed'",
            super::generator_release::verify_attestation_bundles_script()
        ));
    }
    commands.join("\n")
}

fn publish_asset_verification(family: Family) -> String {
    family
        .asset_paths()
        .iter()
        .map(|path| {
            format!(
                "family_gh release verify-asset \"$release_tag\" \"{path}\" --repo \"$release_repository\" >/dev/null\nfamily_gh attestation verify \"{path}\" --repo \"$release_repository\" --source-ref refs/heads/main --source-digest \"$release_source_sha\" --signer-workflow \"$release_repository/$release_workflow_path\" --signer-digest \"$release_authority_sha\" >/dev/null"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn download_patterns(assets: &[String]) -> String {
    assets
        .iter()
        .map(|asset| format!("--pattern '{asset}'"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_words(words: &[String]) -> String {
    words
        .iter()
        .map(|word| format!("'{word}'"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn json_asset_array(assets: &[String]) -> String {
    format!(
        "[{}]",
        assets
            .iter()
            .map(|asset| format!("\"{asset}\""))
            .collect::<Vec<_>>()
            .join(",")
    )
}
