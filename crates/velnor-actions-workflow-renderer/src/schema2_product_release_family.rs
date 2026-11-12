// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::RenderError;
use crate::schema2::ProductReleasePins;
use velnor_actions_contract::ReleaseTarget;

use super::release_eligibility as eligibility;

const PREPARE_TEMPLATE: &str = include_str!("schema2_release_prepare.sh");
const PUBLISH_TEMPLATE: &str = include_str!("schema2_release_publish.sh");
const RELEASE_CONFIRMATION_BUDGET_SECONDS: u16 = 120;

pub(super) const PUBLISH_STEP_ID: &str = "publish-family-release";
pub(super) const ARTIFACT_ID_GUARD_STEP_ID: &str = "validate-producer-artifact-identity";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StepRole {
    Publish,
    ArtifactIdentityGuard,
}

pub(super) fn step_role(id: &str) -> Option<StepRole> {
    match id {
        PUBLISH_STEP_ID => Some(StepRole::Publish),
        ARTIFACT_ID_GUARD_STEP_ID => Some(StepRole::ArtifactIdentityGuard),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    Images,
    Binary,
    Generator,
}

impl Family {
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

    pub(super) const fn prepare_id(self) -> &'static str {
        match self {
            Self::Images => "prepare-images",
            Self::Binary => "prepare-binary",
            Self::Generator => "prepare-generator",
        }
    }

    pub(super) const fn workflow_path(self) -> &'static str {
        match self {
            Self::Images => ".github/workflows/product-release-images.yml",
            Self::Binary => ".github/workflows/product-release-binary.yml",
            Self::Generator => ".github/workflows/product-release-generator.yml",
        }
    }

    pub(super) const fn call_id(self) -> &'static str {
        match self {
            Self::Images => "release-images",
            Self::Binary => "release-binary",
            Self::Generator => "release-generator",
        }
    }

    pub(super) const fn runner_target(self) -> ReleaseTarget {
        match self {
            Self::Images | Self::Generator => ReleaseTarget::LinuxX86_64,
            Self::Binary => ReleaseTarget::MacosArm64,
        }
    }

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Images => "runner images",
            Self::Binary => "velnor-host binary",
            Self::Generator => "velnor-actions generator",
        }
    }

    fn asset_paths(self) -> Vec<String> {
        self.asset_paths_for(false)
    }

    fn asset_paths_for(self, runner: bool) -> Vec<String> {
        if self == Self::Images && runner {
            return [
                "RUNNER_IMAGE_MANIFEST.json",
                "LICENSE",
                "NOTICE",
                "SHA256SUMS",
            ]
            .map(|name| format!("assets/{name}"))
            .to_vec();
        }
        match self {
            Self::Images => [
                "velnor-runner-linux-amd64.tar",
                "velnor-dind-linux-amd64.tar",
                "velnor-resource-probe-linux-amd64.tar",
                "RESOURCE_PROBE_MANIFEST.json",
                "LICENSE",
                "NOTICE",
                "SHA256SUMS",
            ]
            .map(|name| format!("assets/{name}"))
            .to_vec(),
            Self::Binary => [
                "velnor-host",
                "velnor-runner-attestation-helper",
                "LICENSE",
                "NOTICE",
                "SHA256SUMS",
            ]
            .map(|name| format!("assets/{name}"))
            .to_vec(),
            Self::Generator => super::generator_release::publication_asset_paths(),
        }
    }
}

pub(super) fn prepare_script(
    family: Family,
    pins: &ProductReleasePins,
) -> Result<String, RenderError> {
    prepare_script_for(family, pins, false)
}

pub(super) fn runner_prepare_script(pins: &ProductReleasePins) -> Result<String, RenderError> {
    prepare_script_for(Family::Images, pins, true)
}

fn prepare_script_for(
    family: Family,
    pins: &ProductReleasePins,
    runner: bool,
) -> Result<String, RenderError> {
    let checksum = checksum_check(family, "$temp_dir", Some(pins))?;
    let downloads = download_commands(family, runner)?;
    let paths = family.asset_paths_for(runner);
    let names = asset_names(&paths);
    let gh_function = super::generator_release::gh_function(pins)?;
    Ok(PREPARE_TEMPLATE
        .replace("@GH_FUNCTION@", &gh_function)
        .replace("@REPOSITORY@", eligibility::REPOSITORY)
        .replace("@TAG_PREFIX@", family.tag_prefix())
        .replace("@FIXED_TAG@", family.fixed_tag().as_deref().unwrap_or(""))
        .replace("@LABEL@", family.label())
        .replace(
            "@ASSET_NAMES_JSON@",
            &json_asset_array(&names),
        )
        .replace("@DOWNLOAD_COMMANDS@", &downloads)
        .replace("@CHECKSUM_COMMAND@", &checksum)
        .replace(
            "@ASSET_VERIFY_COMMANDS@",
            &if runner {
                format!("{}\n: \"${{VELNOR_FAMILY_ASSETS:?}}\"\nfor name in RUNNER_IMAGE_MANIFEST.json SHA256SUMS LICENSE NOTICE; do cmp -- \"$temp_dir/assets/$name\" \"$VELNOR_FAMILY_ASSETS/$name\" || fail 'existing release differs from authenticated publication'; done", prepare_asset_verification(family, &paths))
            } else { prepare_asset_verification(family, &paths) },
        ))
}

pub(super) fn publish_script(
    family: Family,
    pins: &ProductReleasePins,
) -> Result<String, RenderError> {
    publish_script_for(family, pins, false)
}

pub(super) fn runner_publish_script(pins: &ProductReleasePins) -> Result<String, RenderError> {
    publish_script_for(Family::Images, pins, true)
}

fn publish_script_for(
    family: Family,
    pins: &ProductReleasePins,
    runner: bool,
) -> Result<String, RenderError> {
    if family == Family::Generator {
        return Err(RenderError::InvalidWorkflow(
            "generator_publisher_must_remain_inside_typed_graph".to_owned(),
        ));
    }
    let gh_function = super::generator_release::gh_function(pins)?;
    let paths = family.asset_paths_for(runner);
    let names = asset_names(&paths);
    Ok(PUBLISH_TEMPLATE
        .replace(
            "@CONFIRMATION_GH_FUNCTION@",
            &super::generator_release::gh_confirmation_function(pins)?,
        )
        .replace(
            "@RELEASE_BUDGET_SECONDS@",
            &RELEASE_CONFIRMATION_BUDGET_SECONDS.to_string(),
        )
        .replace("@GH_FUNCTION@", &gh_function)
        .replace(
            "@ELIGIBILITY_FUNCTION@",
            &if runner {
                eligibility::runner_publisher_definition()
            } else {
                eligibility::publisher_definition()
            },
        )
        .replace(
            "@PREPARE_SCRIPT@",
            &prepare_script_for(family, pins, runner)?,
        )
        .replace(
            "@DRAFT_VERIFY@\n",
            if runner { RUNNER_DRAFT_VERIFY } else { "" },
        )
        .replace("@REPOSITORY@", eligibility::REPOSITORY)
        .replace("@WORKFLOW_PATH@", family.workflow_path())
        .replace("@TAG_PREFIX@", family.tag_prefix())
        .replace("@FIXED_TAG@", family.fixed_tag().as_deref().unwrap_or(""))
        .replace("@LABEL@", family.label())
        .replace("@ASSET_NAMES_JSON@", &json_asset_array(&names))
        .replace("@ASSET_PATHS@", &shell_words(&paths))
        .replace(
            "@CHECKSUM_COMMAND@",
            &checksum_check(family, ".", Some(pins))?,
        )
        .replace(
            "@ASSET_VERIFY_COMMANDS@",
            &publish_asset_verification(&paths),
        ))
}

const RUNNER_DRAFT_VERIFY: &str = include_str!("schema2_runner_draft_verify.sh");

fn download_commands(family: Family, runner: bool) -> Result<String, RenderError> {
    match family {
        Family::Images | Family::Binary => Ok(format!(
            "mkdir -p \"$temp_dir/assets\" || fail 'could not create release asset directory'\ngh release download \"$prepare_tag\" --repo \"$repository\" --dir \"$temp_dir/assets\" {} || fail 'release asset download failed'",
            download_patterns(&asset_names(&family.asset_paths_for(runner)))
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
            Ok(format!(
                "(cd \"{root}\" && {})",
                super::generator_release::verify_published_manifest_script(pins)
            ))
        }
    }
}

fn prepare_asset_verification(family: Family, assets: &[String]) -> String {
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
            "(cd \"$temp_dir\" && {})",
            super::generator_release::verify_attestation_bundles_script()
        ));
    }
    commands.join("\n")
}

fn publish_asset_verification(paths: &[String]) -> String {
    paths.iter()
        .map(|path| {
            format!(
                "family_gh release verify-asset \"$release_tag\" \"{path}\" --repo \"$release_repository\" >/dev/null\nfamily_gh attestation verify \"{path}\" --repo \"$release_repository\" --source-ref refs/heads/main --source-digest \"$release_source_sha\" --signer-workflow \"$release_repository/$release_workflow_path\" --signer-digest \"$release_authority_sha\" >/dev/null"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn asset_names(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|path| path.rsplit('/').next().map(str::to_owned))
        .collect()
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
