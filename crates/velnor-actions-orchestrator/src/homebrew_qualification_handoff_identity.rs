// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Admission, Catalog, Context, JSON_LIMIT, QualificationResult, env, is_sha1, is_sha256, number,
    parse_canonical, sha256,
};
use crate::{
    OrchestratorError, homebrew::hosted_profile::VerifiedReleaseQualificationCandidate,
    internal::internal,
};

pub(super) fn context(
    candidate: &VerifiedReleaseQualificationCandidate,
) -> Result<Context, OrchestratorError> {
    let context = Context {
        repository: env("GITHUB_REPOSITORY")?,
        repository_id: number("GITHUB_REPOSITORY_ID")?,
        source_sha: env("GITHUB_SHA")?,
        run_id: number("GITHUB_RUN_ID")?,
        run_attempt: u32::try_from(number("GITHUB_RUN_ATTEMPT")?)
            .map_err(|_| internal("homebrew_handoff_run_attempt"))?,
        workflow_ref: env("GITHUB_WORKFLOW_REF")?,
        workflow_sha: env("GITHUB_WORKFLOW_SHA")?,
        manifest_sha256: candidate.manifest_sha256().to_owned(),
    };
    if context.repository != "tailrocks/velnor-new"
        || !is_sha1(&context.source_sha)
        || !is_sha1(&context.workflow_sha)
        || !context
            .workflow_ref
            .starts_with(&format!("{}/.github/workflows/", context.repository))
        || !context.workflow_ref.ends_with("@refs/heads/main")
    {
        return Err(internal("homebrew_handoff_context"));
    }
    Ok(context)
}

pub(super) fn catalog() -> Catalog {
    Catalog {
        plan: sha256(super::super::PLAN),
        formula: sha256(super::super::FORMULA_SPEC),
        a_record: sha256(super::super::A_RECORD),
        d_record: sha256(super::super::D_RECORD),
        recovery: sha256(super::super::A_ADMISSION),
    }
}

pub(in crate::homebrew::qualification) fn require_credential_free_environment()
-> Result<(), OrchestratorError> {
    if std::env::vars_os().any(|(key, _)| credential_name(&key.to_string_lossy())) {
        return Err(internal("homebrew_handoff_credentials_present"));
    }
    if std::env::var_os("GITHUB_WORKSPACE")
        .is_some_and(|path| !std::path::Path::new(&path).is_absolute())
    {
        return Err(internal("homebrew_handoff_workspace"));
    }
    if std::env::var_os("__CF_USER_TEXT_ENCODING")
        .is_some_and(|value| !valid_cf_encoding(&value, crate::filesystem_owner::effective_uid()))
    {
        return Err(internal("homebrew_handoff_platform_environment"));
    }
    Ok(())
}

pub(super) fn credential_name(name: &str) -> bool {
    !matches!(
        name,
        "PATH"
            | "HOME"
            | "TMPDIR"
            | "GITHUB_ACTIONS"
            | "GITHUB_REPOSITORY"
            | "GITHUB_REPOSITORY_ID"
            | "GITHUB_SHA"
            | "GITHUB_RUN_ID"
            | "GITHUB_RUN_ATTEMPT"
            | "GITHUB_WORKFLOW_REF"
            | "GITHUB_WORKFLOW_SHA"
            | "GITHUB_WORKSPACE"
            | "__CF_USER_TEXT_ENCODING"
            | "RUNNER_ENVIRONMENT"
            | "RUNNER_OS"
            | "RUNNER_ARCH"
            | "RUNNER_TEMP"
            | "VELNOR_INTERNAL_OP"
            | "VELNOR_HOME_BUDGET_DEADLINE_EPOCH"
            | "VELNOR_RELEASE_MANIFEST_SHA256"
            | "VELNOR_HOME_ADMISSION_SHA256"
            | "VELNOR_HOME_ADMISSION_ARCHIVE_SHA256"
            | "VELNOR_HOME_ADMISSION_ARCHIVE_SIZE"
    )
}

pub(super) fn valid_cf_encoding(value: &std::ffi::OsStr, uid: u32) -> bool {
    let Some(value) = value.to_str() else {
        return false;
    };
    let values = value
        .split(':')
        .map(|part| {
            part.strip_prefix("0x")
                .filter(|part| !part.is_empty() && part.len() <= 8)
                .and_then(|part| u32::from_str_radix(part, 16).ok())
        })
        .collect::<Vec<_>>();
    matches!(values.as_slice(), [Some(owner), Some(_), Some(_)] if *owner == uid)
}

pub(super) fn require_unchanged_admission(
    admitted: &Admission,
    fresh: &Admission,
) -> Result<(), OrchestratorError> {
    if fresh != admitted {
        return Err(internal("homebrew_handoff_authority_drift"));
    }
    Ok(())
}

pub(super) fn validate_result(
    bytes: &[u8],
    digest: &str,
    context: &Context,
    admission_digest: &str,
) -> Result<(), OrchestratorError> {
    if bytes.len() as u64 > JSON_LIMIT || !is_sha256(digest) || sha256(bytes) != digest {
        return Err(internal("homebrew_handoff_result_digest"));
    }
    let result: QualificationResult = parse_canonical(bytes)?;
    if result.schema != "velnor.homebrew-qualification-result.v1"
        || &result.context != context
        || result.admission_sha256 != admission_digest
        || result.transition != "A-install-D-upgrade-preserved-kegs"
        || result.stable_sentinel != "disabled"
    {
        return Err(internal("homebrew_handoff_result_identity"));
    }
    Ok(())
}
