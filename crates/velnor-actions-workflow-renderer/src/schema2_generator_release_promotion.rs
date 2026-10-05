//! Manifest preparation, promotion and workflow assembly for generator releases.

use crate::yaml::Yaml;
use velnor_actions_contract::RELEASE_MANIFEST_FILENAME;

use super::super::features::{base, finish};
use super::{AssetNames, LINUX_ARTIFACT, LINUX_DIR, MACOS_ARM_ARTIFACT, MACOS_ARM_DIR};
use super::{MACOS_X64_ARTIFACT, MACOS_X64_DIR, release_steps, scripts};

pub(super) fn prepare_manifest_job(
    hosted: Yaml,
    version: &str,
    assets: &AssetNames,
) -> (String, Yaml) {
    let needs = [
        "release-gate",
        "attest-linux-x64",
        "attest-macos-arm64",
        "attest-macos-x64",
    ];
    let needs_success = needs
        .iter()
        .map(|job| format!("needs.{job}.result == 'success'"))
        .collect::<Vec<_>>()
        .join(" && ");
    let mut fields = with_if(
        with_needs(
            with_permissions(
                base("Prepare and attest release manifest", hosted, 30),
                release_steps::manifest_permissions(),
            ),
            &needs,
        ),
        &needs_success,
    );
    let manifest = format!("release-manifest/{RELEASE_MANIFEST_FILENAME}");
    fields.push((
        "outputs".to_owned(),
        Yaml::Map(vec![(
            "manifest_sha256".to_owned(),
            Yaml::str("${{ steps.manifest-digest.outputs.manifest_sha256 }}"),
        )]),
    ));
    let steps = vec![
        release_steps::checkout_step(),
        release_steps::mise_step(),
        release_steps::download_step("Download Linux x86_64 assets", LINUX_ARTIFACT, LINUX_DIR),
        release_steps::download_step(
            "Download macOS arm64 assets",
            MACOS_ARM_ARTIFACT,
            MACOS_ARM_DIR,
        ),
        release_steps::download_step(
            "Download macOS x86_64 assets",
            MACOS_X64_ARTIFACT,
            MACOS_X64_DIR,
        ),
        release_steps::token_bash_run_step(
            "Verify assets and create canonical same-run manifest",
            &scripts::prepare_manifest(version, assets),
        ),
        release_steps::bash_run_step_with_id(
            "manifest-digest",
            "Record same-run manifest digest",
            &scripts::manifest_digest(&manifest),
        ),
        release_steps::attest_step(&manifest),
        release_steps::upload_step(
            "Upload attested release manifest",
            "generator-release-manifest",
            &[&manifest],
        ),
    ];
    finish("prepare-manifest", fields, steps)
}

pub(super) fn publish_job(hosted: Yaml, version: &str, assets: &AssetNames) -> (String, Yaml) {
    let needs = [
        "release-gate",
        "prepare-manifest",
        "qualify-linux-x64",
        "qualify-macos-arm64",
        "qualify-macos-x64",
    ];
    let needs_success = needs
        .iter()
        .map(|job| format!("needs.{job}.result == 'success'"))
        .collect::<Vec<_>>()
        .join(" && ");
    let mut fields = base("Publish velnor-actions release", hosted, 30);
    fields.push(("environment".to_owned(), Yaml::str("generator-release")));
    let fields = with_if(
        with_needs(
            with_permissions(fields, release_steps::publish_permissions()),
            &needs,
        ),
        &needs_success,
    );
    finish(
        "publish-generator",
        fields,
        vec![
            release_steps::checkout_step(),
            release_steps::mise_step(),
            release_steps::download_step("Download Linux x86_64 assets", LINUX_ARTIFACT, LINUX_DIR),
            release_steps::download_step(
                "Download macOS arm64 assets",
                MACOS_ARM_ARTIFACT,
                MACOS_ARM_DIR,
            ),
            release_steps::download_step(
                "Download macOS x86_64 assets",
                MACOS_X64_ARTIFACT,
                MACOS_X64_DIR,
            ),
            release_steps::download_step(
                "Download attested release manifest",
                "generator-release-manifest",
                "release-manifest",
            ),
            release_steps::publish_bash_step(&scripts::publish(version, assets)),
        ],
    )
}

pub(super) fn with_permissions(
    mut fields: Vec<(String, Yaml)>,
    permissions: Yaml,
) -> Vec<(String, Yaml)> {
    fields.push(("permissions".to_owned(), permissions));
    fields
}

pub(super) fn with_needs(mut fields: Vec<(String, Yaml)>, jobs: &[&str]) -> Vec<(String, Yaml)> {
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(jobs.iter().copied().map(Yaml::str).collect()),
    ));
    fields
}

pub(super) fn with_if(mut fields: Vec<(String, Yaml)>, condition: &str) -> Vec<(String, Yaml)> {
    fields.insert(1, ("if".to_owned(), Yaml::str(condition)));
    fields
}

pub(super) fn document(jobs: Vec<(String, Yaml)>) -> Yaml {
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
