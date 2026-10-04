//! Versioned release manifest assembly and immutable publication.

use crate::yaml::Yaml;

use super::assets::{self, ASSETS, REPOSITORY, VERSION};
use super::finish;
use super::workflow_steps::{self, upload_step, with_needs, with_permissions};

/// Published versioned release manifest.
pub(super) const FILE: &str = "release-manifest.json";
/// Workflow artifact used to carry the attested manifest to publish.
pub(super) const ARTIFACT: &str = "generator-release-manifest";
/// Download location in the publication job.
pub(super) const DIR: &str = "manifest-assets";

/// Verify product sidecars and build exact versioned asset URLs from their digests.
fn manifest_script() -> String {
    let mut lines = vec![
        "set -eu".to_owned(),
        format!("test \"$GITHUB_REPOSITORY\" = \"{REPOSITORY}\""),
        "test \"${#GITHUB_SHA}\" -eq 40".to_owned(),
        "case \"$GITHUB_SHA\" in *[!0123456789abcdef]*|'') exit 1 ;; esac".to_owned(),
    ];
    for asset in ASSETS {
        lines.push(format!(
            "( cd {} && sha256sum --check {} )",
            asset.directory, asset.sidecar
        ));
        lines.push(assets::verify_provenance_script(asset));
        lines.push(format!(
            "{}=\"$(awk -v expected='{}' 'NR == 1 {{ if (NF != 2 || $2 != expected) exit 1; print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}' {}/{})\"",
            asset.digest_var, asset.binary, asset.directory, asset.sidecar
        ));
        lines.push(format!("test \"${{#{}}}\" -eq 64", asset.digest_var));
        lines.push(format!(
            "case \"${}\" in *[!0123456789abcdef]*|'') exit 1 ;; esac",
            asset.digest_var
        ));
    }
    lines.push("tag=\"generator-${GITHUB_SHA}\"".to_owned());
    let records = ASSETS
        .iter()
        .map(|asset| {
            format!(
                r#"{{"target":"{}","artifact":"https://github.com/%s/releases/download/%s/{}","sha256":"%s"}}"#,
                asset.target, asset.binary
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let format = format!(
        r#"{{"schema":1,"version":"{VERSION}","repository":"%s","commit":"%s","targets":[{records}]}}\n"#
    );
    let mut values = vec![
        "\"$GITHUB_REPOSITORY\"".to_owned(),
        "\"$GITHUB_SHA\"".to_owned(),
    ];
    for asset in ASSETS {
        values.extend([
            "\"$GITHUB_REPOSITORY\"".to_owned(),
            "\"$tag\"".to_owned(),
            format!("\"${}\"", asset.digest_var),
        ]);
    }
    lines.push(format!("printf '{format}' {} > {FILE}", values.join(" ")));
    lines.push(format!("test -s {FILE}"));
    lines.push(format!(
        "jq -e --arg version \"{VERSION}\" --arg repository \"$GITHUB_REPOSITORY\" --arg commit \"$GITHUB_SHA\" --arg tag \"$tag\" '{}' {FILE} > /dev/null",
        manifest_filter()
    ));
    lines.join("\n")
}

fn manifest_filter() -> String {
    let mut checks = vec![
        ".schema == 1".to_owned(),
        ".version == $version".to_owned(),
        ".repository == $repository".to_owned(),
        ".commit == $commit".to_owned(),
        format!("(.targets | length) == {}", ASSETS.len()),
    ];
    for (index, asset) in ASSETS.iter().enumerate() {
        checks.push(format!(
            r#".targets[{index}].target == "{}" and .targets[{index}].artifact == ("https://github.com/" + $repository + "/releases/download/" + $tag + "/{}") and (.targets[{index}].sha256 | (length == 64 and test("^[0-9a-f]{{64}}$")))"#,
            asset.target, asset.binary
        ));
    }
    checks.join(" and ")
}

/// Attest the manifest after validating all downloaded product records.
pub(super) fn job(hosted: Yaml) -> (String, Yaml) {
    let needs = ASSETS
        .iter()
        .map(|asset| asset.attest_job)
        .collect::<Vec<_>>();
    let mut steps = ASSETS
        .iter()
        .flat_map(|asset| {
            assets::download_steps(*asset, &format!("Download {} assets", asset.target))
        })
        .collect::<Vec<_>>();
    steps.extend([
        workflow_steps::bash_step(
            "Verify assets and create release manifest",
            &manifest_script(),
        ),
        workflow_steps::attest_step(FILE),
        upload_step("Upload release manifest", ARTIFACT, &[FILE]),
    ]);
    finish(
        "attest-manifest",
        with_needs(
            with_permissions(
                super::base("Attest generator release manifest", hosted, 20),
                workflow_steps::perm(&[
                    ("actions", "write"),
                    ("artifact-metadata", "write"),
                    ("attestations", "write"),
                    ("contents", "read"),
                    ("id-token", "write"),
                ]),
            ),
            &needs,
        ),
        steps,
    )
}

/// Rebuild the expected manifest from verified downloaded binaries and records.
pub(super) fn publication_verify_script() -> String {
    format!("{}\ncmp {FILE} {DIR}/{FILE}", manifest_script())
}

/// Release assets published in stable target order, followed by the manifest.
pub(super) fn release_asset_paths() -> String {
    let mut paths = ASSETS
        .iter()
        .flat_map(|asset| {
            [
                format!("{}/{}", asset.directory, asset.binary),
                format!("{}/{}", asset.directory, asset.sidecar),
                format!("{}/{}", asset.directory, asset.provenance),
            ]
        })
        .collect::<Vec<_>>();
    paths.push(format!("{DIR}/{FILE}"));
    paths.join(" ")
}
