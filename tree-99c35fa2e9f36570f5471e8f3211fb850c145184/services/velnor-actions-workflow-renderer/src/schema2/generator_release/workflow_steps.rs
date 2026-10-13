//! Reusable workflow steps and permission sets for the generator release.

use crate::commands::join_argv_for_run;
use crate::setup::SETUP_MISE_NAME;
use crate::steps::{DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;
use crate::{MiseSetup, RenderError};

use super::super::features::CHECKOUT_USES;

const ATTEST_USES: &str =
    "actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8";

pub(super) fn command_step(name: &str, argv: &[String]) -> Result<Yaml, RenderError> {
    Ok(bash_step(name, &join_argv_for_run(argv)?))
}

pub(super) fn mise_step(setup: &MiseSetup) -> Result<Yaml, RenderError> {
    setup.validate()?;
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(SETUP_MISE_NAME)),
        ("uses".to_owned(), Yaml::str(setup.uses.clone())),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("version".to_owned(), Yaml::str(setup.version.clone())),
                ("sha256".to_owned(), Yaml::str(setup.sha256.clone())),
                ("install".to_owned(), Yaml::str("false")),
                ("env".to_owned(), Yaml::str("false")),
                ("cache".to_owned(), Yaml::str("false")),
                ("cache_save".to_owned(), Yaml::str("false")),
            ]),
        ),
    ]))
}

pub(super) fn gh_function(argv: &[String]) -> Result<String, RenderError> {
    let executable = join_argv_for_run(argv)?;
    Ok(format!("gh() {{ {executable} \"$@\"; }}\nexport -f gh"))
}

pub(super) fn install_gh_step(install_argv: &[String]) -> Result<Yaml, RenderError> {
    command_step("Install pinned GitHub CLI", install_argv)
}
pub(super) fn bash_step(name: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

pub(super) fn bash_step_with_id(id: &str, name: &str, run: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(id)),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

/// Run one API step with only the repository's read-scoped token.
pub(super) fn bash_step_with_env(name: &str, run: &str, env: Vec<(&str, &str)>) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        (
            "env".to_owned(),
            Yaml::Map(
                env.into_iter()
                    .map(|(key, value)| (key.to_owned(), Yaml::str(value)))
                    .collect(),
            ),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

pub(super) fn bash_step_with_token(
    name: &str,
    run: &str,
    gh_argv: &[String],
) -> Result<Yaml, RenderError> {
    let full_run = format!("{}\n{run}", gh_function(gh_argv)?);
    Ok(bash_step_with_env(
        name,
        &full_run,
        vec![("GH_TOKEN", "${{ github.token }}")],
    ))
}

pub(super) fn publish_step(run: &str, gh_argv: &[String]) -> Result<Yaml, RenderError> {
    let full_run = format!("{}\n{run}", gh_function(gh_argv)?);
    Ok(bash_step_with_env(
        "Publish GitHub release",
        &full_run,
        vec![("GH_TOKEN", "${{ github.token }}")],
    ))
}

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
    upload_step_inner(None, name, artifact, files)
}

pub(super) fn upload_step_with_id(id: &str, name: &str, artifact: &str, files: &[&str]) -> Yaml {
    upload_step_inner(Some(id), name, artifact, files)
}

fn upload_step_inner(id: Option<&str>, name: &str, artifact: &str, files: &[&str]) -> Yaml {
    let mut fields = vec![("name".to_owned(), Yaml::str(name))];
    if let Some(id) = id {
        fields.push(("id".to_owned(), Yaml::str(id)));
    }
    fields.extend([
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
    ]);
    Yaml::Map(fields)
}

pub(super) fn download_step(name: &str, artifact: &str, path: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("name".to_owned(), Yaml::str(artifact)),
                ("path".to_owned(), Yaml::str(path)),
            ]),
        ),
    ])
}

pub(super) fn download_step_by_id(name: &str, artifact_id: &str, path: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("uses".to_owned(), Yaml::str(DOWNLOAD_ARTIFACT_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("artifact-ids".to_owned(), Yaml::str(artifact_id)),
                ("path".to_owned(), Yaml::str(path)),
            ]),
        ),
    ])
}

pub(super) fn attest_step(subjects: &str) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Attest built artifacts")),
        ("uses".to_owned(), Yaml::str(ATTEST_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![("subject-path".to_owned(), Yaml::str(subjects))]),
        ),
    ])
}

pub(super) fn with_permissions(
    mut fields: Vec<(String, Yaml)>,
    permissions: Yaml,
) -> Vec<(String, Yaml)> {
    fields.push(("permissions".to_owned(), permissions));
    fields
}

pub(super) fn with_needs(mut fields: Vec<(String, Yaml)>, needs: &[&str]) -> Vec<(String, Yaml)> {
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(needs.iter().copied().map(Yaml::str).collect()),
    ));
    fields
}

/// Build uploads a workflow artifact. That needs `actions: write`, not contents write.
pub(super) fn build_permissions() -> Yaml {
    perm(&[("actions", "write"), ("contents", "read")])
}

/// Attestation uploads bundle artifacts but cannot write repository contents.
pub(super) fn attest_permissions() -> Yaml {
    perm(&[
        ("actions", "write"),
        ("artifact-metadata", "write"),
        ("attestations", "write"),
        ("contents", "read"),
        ("id-token", "write"),
    ])
}

/// Qualification needs no token because it fetches the public source by commit.
pub(super) fn qualification_permissions() -> Yaml {
    Yaml::Map(Vec::new())
}

/// The publisher uploads one accepted-metadata artifact and writes the release.
pub(super) fn publish_permissions() -> Yaml {
    perm(&[("actions", "write"), ("contents", "write")])
}

pub(super) fn perm(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

pub(super) fn subject_list(files: &[&str], directory: &str) -> String {
    files
        .iter()
        .map(|file| format!("{directory}/{file}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn newline_list(files: &[&str]) -> String {
    files.join("\n")
}

#[cfg(test)]
mod tests;
