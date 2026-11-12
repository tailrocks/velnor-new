// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, render::RenderedFile, yaml::Yaml};
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use velnor_actions_contract::config::PremergeEventProofConfig;

pub const WORKFLOW_PATH: &str = ".github/workflows/premerge-event-proof.yml";

pub fn render(
    config: &PremergeEventProofConfig,
    repository: &str,
    branch: &str,
    version: &str,
) -> Result<RenderedFile, RenderError> {
    config
        .validate(".velnor/config.toml")
        .map_err(|e| RenderError::InvalidWorkflow(e.to_string()))?;
    if !velnor_actions_contract::vcs::is_safe_github_repository(repository)
        || !velnor_actions_contract::is_valid_branch_name(branch)
    {
        return Err(RenderError::InvalidWorkflow(
            "premerge_event_proof_identity".into(),
        ));
    }
    let job = Yaml::Map(vec![
        ("name".into(), Yaml::str("Attest premerge event")),
        ("runs-on".into(), Yaml::str("ubuntu-26.04")),
        ("timeout-minutes".into(), Yaml::Int(10)),
        ("environment".into(), Yaml::str(&config.environment)),
        (
            "permissions".into(),
            strings(&[
                ("actions", "read"),
                ("contents", "read"),
                ("pull-requests", "read"),
                ("id-token", "write"),
                ("attestations", "write"),
            ]),
        ),
        (
            "steps".into(),
            Yaml::Seq(producer_steps(config, repository, branch)),
        ),
    ]);
    let document = Yaml::Map(vec![
        ("name".into(), Yaml::str("Premerge event proof")),
        (
            "on".into(),
            Yaml::Map(vec![("workflow_call".into(), Yaml::Map(vec![]))]),
        ),
        ("permissions".into(), strings(&[("contents", "read")])),
        ("jobs".into(), Yaml::Map(vec![("attest".into(), job)])),
    ]);
    let bytes =
        crate::render::fallback::render_checked_workflow(WORKFLOW_PATH, &document, version)?;
    Ok(RenderedFile {
        path: WORKFLOW_PATH.into(),
        bytes,
    })
}

fn producer_steps(config: &PremergeEventProofConfig, repository: &str, branch: &str) -> Vec<Yaml> {
    let source = format!(
        "{}\nsys.exit(main())\n",
        include_str!("premerge_event_probe.py")
    );
    let chunks = source.as_bytes().chunks(9000);
    let count = chunks.len();
    let mut steps = chunks
        .enumerate()
        .map(|(index, chunk)| {
            run_step(
                format!("Materialize premerge authority {index}"),
                source_command(&format!(
                    "write_chunk({index}, '{}')",
                    STANDARD.encode(chunk)
                )),
                None,
            )
        })
        .collect::<Vec<_>>();
    let digest = Sha256::digest(source.as_bytes())
        .iter()
        .flat_map(|b| {
            [
                char::from(b"0123456789abcdef"[usize::from(b >> 4)]),
                char::from(b"0123456789abcdef"[usize::from(b & 15)]),
            ]
        })
        .collect::<String>();
    steps.push(run_step(
        "Observe and authorize premerge event".into(),
        source_command(&format!("execute_source({count}, '{digest}')")),
        Some(producer_environment(config, repository, branch)),
    ));
    steps.push(Yaml::Map(vec![
        ("name".into(), Yaml::str("Attest observed premerge event")),
        (
            "uses".into(),
            Yaml::str(crate::steps::ATTEST_BUILD_PROVENANCE_USES),
        ),
        (
            "with".into(),
            strings(&[(
                "subject-path",
                "${{ env.VELNOR_PREMERGE_SOURCE }}/receipt.json",
            )]),
        ),
    ]));
    steps.push(Yaml::Map(vec![
        ("name".into(), Yaml::str("Upload attested premerge event")),
        ("uses".into(), Yaml::str(crate::steps::UPLOAD_ARTIFACT_USES)),
        (
            "with".into(),
            strings(&[
                (
                    "name",
                    "premerge-event-r${{ github.run_id }}-a${{ github.run_attempt }}",
                ),
                ("path", "${{ env.VELNOR_PREMERGE_SOURCE }}/receipt.json"),
                ("if-no-files-found", "error"),
                ("retention-days", "90"),
            ]),
        ),
    ]));
    steps
}

fn producer_environment(config: &PremergeEventProofConfig, repository: &str, branch: &str) -> Yaml {
    let mut fields = vec![
        ("GIT_TOKEN", "${{ github.token }}".to_owned()),
        ("VELNOR_PROOF_REPOSITORY", repository.to_owned()),
        (
            "VELNOR_PROOF_REPOSITORY_ID",
            config.repository_id.to_string(),
        ),
        ("VELNOR_PROOF_OWNER_ID", config.owner_id.to_string()),
        ("VELNOR_PROOF_BASE_BRANCH", branch.to_owned()),
        (
            "VELNOR_PROOF_CALLER_WORKFLOW",
            config.caller_workflow.clone(),
        ),
        ("VELNOR_PROOF_ENVIRONMENT", config.environment.clone()),
        ("VELNOR_PROOF_REVIEWER_ID", config.reviewer_id.to_string()),
    ];
    fields.extend([
        (
            "VELNOR_PROOF_SIGNER_SHA",
            "${{ job.workflow_sha }}".to_owned(),
        ),
        (
            "VELNOR_PROOF_SIGNER_REF",
            "${{ job.workflow_ref }}".to_owned(),
        ),
        (
            "VELNOR_PROOF_SIGNER_REPOSITORY",
            "${{ job.workflow_repository }}".to_owned(),
        ),
        (
            "VELNOR_PROOF_SIGNER_PATH",
            "${{ job.workflow_file_path }}".to_owned(),
        ),
        ("VELNOR_PROOF_JOB_ID", "${{ job.check_run_id }}".to_owned()),
    ]);
    Yaml::Map(
        fields
            .into_iter()
            .map(|(k, v)| (k.to_owned(), Yaml::str(v)))
            .collect(),
    )
}

fn run_step(name: String, command: String, environment: Option<Yaml>) -> Yaml {
    let mut fields = vec![
        ("name".into(), Yaml::str(name)),
        (
            "shell".into(),
            Yaml::str(crate::root_api::VERIFY_VERIFIER_SHELL),
        ),
        ("run".into(), Yaml::str(command)),
    ];
    if let Some(environment) = environment {
        fields.push(("env".into(), environment));
    }
    Yaml::Map(fields)
}

fn source_command(operation: &str) -> String {
    format!(
        "set -eu\numask 077\nunset BASH_ENV ENV SSLKEYLOGFILE PYTHONHOME PYTHONPATH PYTHONSTARTUP PYTHONINSPECT PYTHONUSERBASE PYTHONBREAKPOINT PYTHONWARNINGS PYTHONPYCACHEPREFIX PYTHONEXECUTABLE LD_PRELOAD LD_LIBRARY_PATH DYLD_INSERT_LIBRARIES DYLD_LIBRARY_PATH SSL_CERT_FILE SSL_CERT_DIR REQUESTS_CA_BUNDLE CURL_CA_BUNDLE http_proxy https_proxy all_proxy HTTP_PROXY HTTPS_PROXY ALL_PROXY\n/usr/bin/python3 -I -S - <<'VELNOR_PREMERGE_SOURCE_PY'\n{}\n{operation}\nVELNOR_PREMERGE_SOURCE_PY",
        include_str!("premerge_source.py")
    )
}

fn strings(values: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        values
            .iter()
            .map(|(k, v)| ((*k).to_owned(), Yaml::str(*v)))
            .collect(),
    )
}
