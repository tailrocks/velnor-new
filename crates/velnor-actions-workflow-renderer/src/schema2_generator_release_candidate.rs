//! Read-only pull-request qualification of one exact three-target candidate.

use super::super::Schema2WorkflowRequest;
use super::{
    AssetNames, LINUX_TARGET, MACOS_ARM_TARGET, MACOS_RUNS_ON, MACOS_X64_RUNS_ON, MACOS_X64_TARGET,
    MISE_USES, MISE_VERSION,
};
use crate::RenderError;
use crate::runs_on::runs_on_yaml;
use crate::yaml::Yaml;
use velnor_actions_contract::RELEASE_MANIFEST_FILENAME;

#[path = "schema2_generator_release_candidate_jobs.rs"]
mod jobs;

const CANDIDATE_LINUX_ARTIFACT_PREFIX: &str = "generator-pr-candidate-linux-x64-assets";
const CANDIDATE_MACOS_ARM_ARTIFACT_PREFIX: &str = "generator-pr-candidate-macos-arm64-assets";
const CANDIDATE_MACOS_X64_ARTIFACT_PREFIX: &str = "generator-pr-candidate-macos-x64-assets";
const CANDIDATE_MANIFEST_ARTIFACT_PREFIX: &str = "generator-pr-candidate-release-manifest";
const CANDIDATE_MANIFEST_DIR: &str = "manifest-assets";
const CANDIDATE_ASSET_DIR: &str = "assets";

struct CandidateTarget {
    id: &'static str,
    name: &'static str,
    build_id: &'static str,
    artifact: String,
    target: &'static str,
    asset: String,
    sidecar: String,
    provenance: String,
    build_runner: Yaml,
    qualify_runner: Yaml,
    os: &'static str,
}

/// Render a pull-request-only, read-only candidate build and qualification graph.
pub(super) fn workflow(request: &Schema2WorkflowRequest) -> Result<Yaml, RenderError> {
    let hosted = runs_on_yaml(&request.hosted_label)?;
    let macos_arm = runs_on_yaml(MACOS_RUNS_ON)?;
    let macos_x64 = runs_on_yaml(MACOS_X64_RUNS_ON)?;
    let assets = AssetNames::for_version(&request.version);
    let targets = targets(hosted.clone(), &macos_arm, &macos_x64, &assets);
    let mut workflow_jobs = vec![jobs::gate_job(hosted.clone())];
    workflow_jobs.extend(
        targets
            .iter()
            .map(|target| jobs::build_job(target, &request.version)),
    );
    workflow_jobs.push(jobs::manifest_job(hosted, &request.version));
    workflow_jobs.extend(
        targets
            .iter()
            .map(|target| jobs::qualify_job(target, &request.version)),
    );
    let build_ids = targets
        .iter()
        .map(|target| target.build_id)
        .collect::<Vec<_>>();
    workflow_jobs.push(jobs::required_job(
        runs_on_yaml(&request.hosted_label)?,
        &build_ids,
    ));
    Ok(document(workflow_jobs))
}

fn targets(
    hosted: Yaml,
    macos_arm: &Yaml,
    macos_x64: &Yaml,
    assets: &AssetNames,
) -> [CandidateTarget; 3] {
    [
        CandidateTarget {
            id: "candidate-build-linux-x64",
            name: "Build pull-request candidate / Linux x86_64",
            build_id: "candidate-build-linux-x64",
            artifact: scoped_artifact_name(CANDIDATE_LINUX_ARTIFACT_PREFIX),
            target: LINUX_TARGET,
            asset: assets.linux_bin.clone(),
            sidecar: assets.linux_sum.clone(),
            provenance: assets.linux_provenance.clone(),
            build_runner: hosted.clone(),
            qualify_runner: hosted,
            os: "linux",
        },
        CandidateTarget {
            id: "candidate-build-macos-arm64",
            name: "Build pull-request candidate / macOS arm64",
            build_id: "candidate-build-macos-arm64",
            artifact: scoped_artifact_name(CANDIDATE_MACOS_ARM_ARTIFACT_PREFIX),
            target: MACOS_ARM_TARGET,
            asset: assets.macos_arm_bin.clone(),
            sidecar: assets.macos_arm_sum.clone(),
            provenance: assets.macos_arm_provenance.clone(),
            build_runner: macos_arm.clone(),
            qualify_runner: macos_arm.clone(),
            os: "macos-arm64",
        },
        CandidateTarget {
            id: "candidate-build-macos-x64",
            name: "Build pull-request candidate / macOS x86_64",
            build_id: "candidate-build-macos-x64",
            artifact: scoped_artifact_name(CANDIDATE_MACOS_X64_ARTIFACT_PREFIX),
            target: MACOS_X64_TARGET,
            asset: assets.macos_x64_bin.clone(),
            sidecar: assets.macos_x64_sum.clone(),
            provenance: assets.macos_x64_provenance.clone(),
            build_runner: (*macos_x64).clone(),
            qualify_runner: (*macos_x64).clone(),
            os: "macos-x64",
        },
    ]
}

fn scoped_artifact_name(prefix: &str) -> String {
    format!("{prefix}-run-${{{{ github.run_id }}}}-attempt-${{{{ github.run_attempt }}}}")
}

pub(super) fn run_context_outputs() -> Vec<(String, Yaml)> {
    ["run_id", "run_attempt", "source_sha"]
        .iter()
        .map(|name| {
            (
                (*name).to_owned(),
                Yaml::str(format!("${{{{ steps.candidate-context.outputs.{name} }}}}")),
            )
        })
        .collect()
}

pub(super) fn source_gate_script() -> String {
    format!(
        r#"set -eu
test "$GITHUB_EVENT_NAME" = "pull_request"
test "$GITHUB_REPOSITORY" = "tailrocks/velnor-new"
[[ "$VELNOR_PR_NUMBER" =~ ^[1-9][0-9]*$ ]]
test "$GITHUB_REF" = "refs/pull/$VELNOR_PR_NUMBER/merge"
test "$GITHUB_WORKFLOW_REF" = "$GITHUB_REPOSITORY/{}@$GITHUB_REF"
test "$GITHUB_WORKFLOW_SHA" = "$GITHUB_SHA"
[[ "$GITHUB_SHA" =~ ^[0-9a-f]{{40}}$ ]]
test "$(git rev-parse HEAD)" = "$GITHUB_SHA"
[[ "$GITHUB_RUN_ID" =~ ^[1-9][0-9]*$ ]]
[[ "$GITHUB_RUN_ATTEMPT" =~ ^[1-9][0-9]*$ ]]
{{
  printf 'run_id=%s\n' "$GITHUB_RUN_ID"
  printf 'run_attempt=%s\n' "$GITHUB_RUN_ATTEMPT"
  printf 'source_sha=%s\n' "$GITHUB_SHA"
}} >> "$GITHUB_OUTPUT"
"#,
        super::super::GENERATOR_CANDIDATE_QUALIFICATION_WORKFLOW
    )
}

fn manifest_script(version: &str) -> String {
    format!(
        r#"set -eu
catalog_version() {{
  awk -F '"' -v name="$1" '$1 == "pub const " name ": &str = " && $3 == ";" {{ value = $2; count += 1 }} END {{ if (count != 1 || value !~ /^[0-9]+\.[0-9]+\.[0-9]+$/) exit 1; print value }}' crates/velnor-actions-mise/src/catalog.rs
}}
test "$GITHUB_REPOSITORY" = "tailrocks/velnor-new"
test "$GITHUB_WORKFLOW_SHA" = "$GITHUB_SHA"
test "$(git rev-parse HEAD)" = "$GITHUB_SHA"
if [ -L release-manifest.json ]; then
  echo 'candidate manifest output must not be a symlink' >&2
  exit 1
fi
rust_version="$(catalog_version RUST_VERSION)"
mr_boxington_version="$(catalog_version MR_BOXINGTON_VERSION)"
bash scripts/generator-release/create-release-manifest.sh "{version}" "tailrocks/velnor-new" "$rust_version" "$mr_boxington_version"
if [ ! -f release-manifest.json ] || [ -L release-manifest.json ] || [ ! -s release-manifest.json ]; then
  echo 'candidate manifest producer must write a non-empty regular file' >&2
  exit 1
fi
if [ -L "{CANDIDATE_MANIFEST_DIR}" ] || {{ [ -e "{CANDIDATE_MANIFEST_DIR}" ] && [ ! -d "{CANDIDATE_MANIFEST_DIR}" ]; }}; then
  echo 'candidate manifest artifact directory must be a real directory' >&2
  exit 1
fi
mkdir -p "{CANDIDATE_MANIFEST_DIR}"
if [ -L "{CANDIDATE_MANIFEST_DIR}/{RELEASE_MANIFEST_FILENAME}" ]; then
  echo 'candidate manifest artifact file must not be a symlink' >&2
  exit 1
fi
mv release-manifest.json "{CANDIDATE_MANIFEST_DIR}/{RELEASE_MANIFEST_FILENAME}"
if [ ! -f "{CANDIDATE_MANIFEST_DIR}/{RELEASE_MANIFEST_FILENAME}" ] || [ -L "{CANDIDATE_MANIFEST_DIR}/{RELEASE_MANIFEST_FILENAME}" ] || [ ! -s "{CANDIDATE_MANIFEST_DIR}/{RELEASE_MANIFEST_FILENAME}" ]; then
  echo 'candidate manifest artifact must be a non-empty regular file' >&2
  exit 1
fi
"#
    )
}

fn manifest_digest_script() -> String {
    format!(
        r#"set -euo pipefail
digest="$(sha256sum "{CANDIDATE_MANIFEST_DIR}/{RELEASE_MANIFEST_FILENAME}" | awk 'NR == 1 && length($1) == 64 && $1 !~ /[^0-9a-f]/ {{ print $1; next }} {{ exit 1 }} END {{ if (NR != 1) exit 1 }}')"
printf 'manifest_sha256=%s\n' "$digest" >> "$GITHUB_OUTPUT"
"#
    )
}

fn download_build_artifact(name: &str, build_id: &str, path: &str) -> Yaml {
    super::release_steps::download_artifact_id_step(
        name,
        &format!("${{{{ needs.{build_id}.outputs.asset_id }}}}"),
        path,
    )
}

fn mise_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Setup pinned Mise")),
        ("uses".to_owned(), Yaml::str(MISE_USES)),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                ("cache".to_owned(), Yaml::Bool(false)),
                ("env".to_owned(), Yaml::Bool(false)),
                ("install".to_owned(), Yaml::Bool(false)),
                ("version".to_owned(), Yaml::str(MISE_VERSION)),
            ]),
        ),
    ])
}

fn document(jobs: Vec<(String, Yaml)>) -> Yaml {
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Generator candidate qualification"),
        ),
        (
            "on".to_owned(),
            Yaml::Map(vec![(
                "pull_request".to_owned(),
                Yaml::Map(vec![
                    ("branches".to_owned(), Yaml::Seq(vec![Yaml::str("main")])),
                    (
                        "types".to_owned(),
                        Yaml::Seq(vec![
                            Yaml::str("opened"),
                            Yaml::str("synchronize"),
                            Yaml::str("reopened"),
                            Yaml::str("ready_for_review"),
                        ]),
                    ),
                ]),
            )]),
        ),
        ("permissions".to_owned(), read_permissions()),
        ("jobs".to_owned(), Yaml::Map(jobs)),
    ])
}

fn read_permissions() -> Yaml {
    Yaml::Map(vec![
        ("actions".to_owned(), Yaml::str("read")),
        ("contents".to_owned(), Yaml::str("read")),
    ])
}

fn build_permissions() -> Yaml {
    Yaml::Map(vec![("contents".to_owned(), Yaml::str("read"))])
}

fn with_if(mut fields: Vec<(String, Yaml)>, condition: &str) -> Vec<(String, Yaml)> {
    fields.insert(1, ("if".to_owned(), Yaml::str(condition)));
    fields
}

fn with_needs(mut fields: Vec<(String, Yaml)>, jobs: &[&str]) -> Vec<(String, Yaml)> {
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(jobs.iter().copied().map(Yaml::str).collect()),
    ));
    fields
}

fn with_permissions(mut fields: Vec<(String, Yaml)>, permissions: Yaml) -> Vec<(String, Yaml)> {
    fields.push(("permissions".to_owned(), permissions));
    fields
}

pub(super) fn needs_success(jobs: &[&str]) -> String {
    jobs.iter()
        .flat_map(|job| {
            [
                format!("needs.{job}.result == 'success'"),
                format!("needs.{job}.outputs.run_id == github.run_id"),
                format!("needs.{job}.outputs.run_attempt == github.run_attempt"),
                format!("needs.{job}.outputs.source_sha == github.sha"),
            ]
        })
        .collect::<Vec<_>>()
        .join(" && ")
}

pub(super) fn candidate_context_step() -> Yaml {
    super::release_steps::bash_run_step_with_id_env(
        "candidate-context",
        "Bind workflow authority to merge candidate source",
        &source_gate_script(),
        "VELNOR_PR_NUMBER",
        "${{ github.event.pull_request.number }}",
    )
}

pub(super) fn manifest_artifact_name() -> String {
    scoped_artifact_name(CANDIDATE_MANIFEST_ARTIFACT_PREFIX)
}
