// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    ARTIFACT_ID_GUARD_STEP_ID, ASSET_DIR, ATTEST_BUILD_PROVENANCE_USES, CHECKOUT_USES,
    DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES, UPLOAD_ASSETS_STEP_ID, Yaml,
};

pub(super) fn checkout_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out")),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("fetch-depth".to_owned(), Yaml::str("1")),
                ("persist-credentials".to_owned(), Yaml::str("false")),
            ]),
        ),
    ])
}

pub(super) fn upload_step(name: &str, artifact: &str, files: &[&str]) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(UPLOAD_ASSETS_STEP_ID)),
        ("uses".to_owned(), Yaml::str(UPLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("if-no-files-found".to_owned(), Yaml::str("error")),
                ("name".to_owned(), Yaml::str(artifact)),
                ("path".to_owned(), Yaml::str(newline_list(files))),
                ("retention-days".to_owned(), Yaml::Int(1)),
            ]),
        ),
    ])
}

pub(super) fn validate_artifact_id_step(artifact_id: &str) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Validate producer artifact identity"),
        ),
        ("id".to_owned(), Yaml::str(ARTIFACT_ID_GUARD_STEP_ID)),
        (
            "env".to_owned(),
            Yaml::Map(vec![("ARTIFACT_ID".to_owned(), Yaml::str(artifact_id))]),
        ),
        (
            "run".to_owned(),
            Yaml::str(
                "set -eu\nexport LC_ALL=C\ncase \"$ARTIFACT_ID\" in ''|*[!0-9]*|0|0*) exit 1 ;; esac\nlength=\"${#ARTIFACT_ID}\"\nif [ \"$length\" -gt 16 ]; then exit 1; fi\nif [ \"$length\" -eq 16 ] && [ \"$ARTIFACT_ID\" -gt 9007199254740991 ]; then exit 1; fi",
            ),
        ),
    ])
}

pub(super) fn download_by_id_step(artifact_id: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Download built assets")),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "env".to_owned(),
            Yaml::Map(vec![("ARTIFACT_ID".to_owned(), Yaml::str(artifact_id))]),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                (
                    "artifact-ids".to_owned(),
                    Yaml::str("${{ env.ARTIFACT_ID }}"),
                ),
                ("path".to_owned(), Yaml::str(ASSET_DIR)),
            ]),
        ),
    ])
}

pub(super) fn attest_step(subjects: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Attest built artifacts")),
        ("uses".to_owned(), Yaml::str(ATTEST_BUILD_PROVENANCE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![("subject-path".to_owned(), Yaml::str(subjects))]),
        ),
    ])
}

pub(super) fn with_permissions(
    mut fields: Vec<(String, Yaml)>,
    perms: Yaml,
) -> Vec<(String, Yaml)> {
    fields.push(("permissions".to_owned(), perms));
    fields
}

pub(super) fn with_needs(mut fields: Vec<(String, Yaml)>, needs: &str) -> Vec<(String, Yaml)> {
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(vec![Yaml::str(needs.to_owned())]),
    ));
    fields
}

pub(super) fn build_permissions() -> Yaml {
    perm(&[("actions", "write"), ("contents", "read")])
}

pub(super) fn attest_permissions() -> Yaml {
    perm(&[
        ("actions", "read"),
        ("artifact-metadata", "write"),
        ("attestations", "write"),
        ("contents", "read"),
        ("id-token", "write"),
    ])
}

pub(super) fn publish_permissions() -> Yaml {
    perm(&[("actions", "read"), ("contents", "write")])
}

pub(super) fn perm(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

pub(super) fn subject_list(files: &[&str]) -> String {
    files
        .iter()
        .map(|file| format!("{ASSET_DIR}/{file}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn newline_list(files: &[&str]) -> String {
    files.join("\n")
}

pub(super) fn release_command(prefix: &str, notes: &str, files: &[&str]) -> String {
    format!(
        "set -eu\ncd {ASSET_DIR}\ntag=\"{prefix}-${{GITHUB_SHA}}\"\ngh release create \"$tag\" -R \"${{GITHUB_REPOSITORY}}\" --target \"$GITHUB_SHA\" --title \"$tag\" --latest=false --notes \"{notes}\" {}",
        files.join(" ")
    )
}

pub(super) fn document(name: &str, jobs: Vec<(String, Yaml)>) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
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
