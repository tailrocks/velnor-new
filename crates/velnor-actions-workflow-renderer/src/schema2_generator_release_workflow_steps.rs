// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::commands::join_argv_for_run;
use crate::setup::SETUP_MISE_NAME;
use crate::steps::{ATTEST_BUILD_PROVENANCE_USES, DOWNLOAD_ARTIFACT_USES, UPLOAD_ARTIFACT_USES};
use crate::yaml::Yaml;
use crate::{MiseSetup, RenderError};

use super::super::features::CHECKOUT_USES;

const GH_COMMAND_TIMEOUT_SECONDS: u16 = 60;
const GH_COMMAND_KILL_AFTER_SECONDS: u16 = 5;

pub(super) fn command_step(name: &str, argv: &[String]) -> Result<Yaml, RenderError> {
    if matches!(
        name,
        crate::VERIFY_RUST_TOOLCHAIN_STEP | "Verify jactionlint"
    ) {
        return crate::verifier_shell::render_command(name, argv);
    }
    Ok(bash_step(name, &join_argv_for_run(argv)?))
}

pub(super) fn verify_rust_step(pins: &super::ProductReleasePins) -> Result<Yaml, RenderError> {
    command_step(crate::VERIFY_RUST_TOOLCHAIN_STEP, &pins.verify_rust_argv)
}

pub(super) fn mise_install_step(name: &str, argv: &[String]) -> Result<Yaml, RenderError> {
    let command = join_argv_for_run(argv)?;
    let run = format!("set -eu\n: \"${{GITHUB_TOKEN:?missing workflow token}}\"\n{command}");
    Ok(bash_step_with_env(
        name,
        &run,
        vec![("GITHUB_TOKEN", "${{ github.token }}")],
    ))
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
    gh_function_with_timeout(argv, GH_COMMAND_TIMEOUT_SECONDS)
}

pub(super) fn gh_confirmation_function(argv: &[String]) -> Result<String, RenderError> {
    let executable = join_argv_for_run(argv)?;
    Ok(r#"gh_until() {
  local _velnor_deadline="$1"
  shift
  (
    _velnor_remaining=$((_velnor_deadline - SECONDS))
    (( _velnor_remaining > 0 )) || exit 124
    _velnor_timeout=@TIMEOUT@
    _velnor_kill_after=@KILL_AFTER@
    if (( _velnor_remaining < _velnor_timeout + _velnor_kill_after )); then
      _velnor_kill_after=$(( _velnor_remaining < _velnor_kill_after ? _velnor_remaining : _velnor_kill_after ))
      _velnor_timeout=$((_velnor_remaining - _velnor_kill_after))
    fi
    @EXECUTABLE@ "$@" & _velnor_gh_pid=$!
    ( sleep "$_velnor_timeout"; kill -TERM "$_velnor_gh_pid" 2>/dev/null; sleep "$_velnor_kill_after"; kill -KILL "$_velnor_gh_pid" 2>/dev/null ) </dev/null >/dev/null 2>&1 & _velnor_gh_watch=$!
    _velnor_gh_status=0
    wait "$_velnor_gh_pid" || _velnor_gh_status=$?
    kill -KILL "$_velnor_gh_watch" 2>/dev/null || true
    wait "$_velnor_gh_watch" 2>/dev/null || true
    (( SECONDS < _velnor_deadline )) || exit 124
    exit "$_velnor_gh_status"
  ) </dev/null
}"#
        .replace("@EXECUTABLE@", &executable)
        .replace("@TIMEOUT@", &GH_COMMAND_TIMEOUT_SECONDS.to_string())
        .replace("@KILL_AFTER@", &GH_COMMAND_KILL_AFTER_SECONDS.to_string()))
}

fn gh_function_with_timeout(argv: &[String], timeout_seconds: u16) -> Result<String, RenderError> {
    let executable = join_argv_for_run(argv)?;
    let kill_after_seconds = GH_COMMAND_KILL_AFTER_SECONDS;
    Ok(format!(
        "gh() {{\n  (\n    {executable} \"$@\" & _velnor_gh_pid=$!\n    ( sleep {timeout_seconds}; kill -TERM \"$_velnor_gh_pid\" 2>/dev/null; sleep {kill_after_seconds}; kill -KILL \"$_velnor_gh_pid\" 2>/dev/null ) </dev/null >/dev/null 2>&1 & _velnor_gh_watch=$!\n    _velnor_gh_status=0\n    wait \"$_velnor_gh_pid\" || _velnor_gh_status=$?\n    kill -KILL \"$_velnor_gh_watch\" 2>/dev/null || true\n    wait \"$_velnor_gh_watch\" 2>/dev/null || true\n    exit \"$_velnor_gh_status\"\n  ) </dev/null\n}}\nexport -f gh"
    ))
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
        ("uses".to_owned(), Yaml::str(ATTEST_BUILD_PROVENANCE_USES)),
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

pub(super) fn build_permissions() -> Yaml {
    perm(&[("actions", "write"), ("contents", "read")])
}

pub(super) fn attest_permissions() -> Yaml {
    perm(&[
        ("actions", "write"),
        ("artifact-metadata", "write"),
        ("attestations", "write"),
        ("contents", "read"),
        ("id-token", "write"),
    ])
}

pub(super) fn qualification_permissions() -> Yaml {
    Yaml::Map(Vec::new())
}

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
