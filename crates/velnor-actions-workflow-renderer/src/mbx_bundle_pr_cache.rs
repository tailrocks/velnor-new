//! Runtime validation and key isolation for opt-in same-repository PR caches.

use std::collections::BTreeMap;

use velnor_actions_contract::{PullRequestCachePolicy, Step};

use crate::RenderError;

pub(super) const PR_CACHE_ALLOWED_OUTPUT: &str = "steps.mbx-bundle-key.outputs.pr-cache-allowed";

/// Validated child key fields plus receipt values used only as equality guards.
#[derive(Clone, Copy)]
pub(crate) struct QualificationObserverBinding<'a> {
    pub child_run_id: &'a str,
    pub child_attempt: &'a str,
    pub source_sha: &'a str,
    pub receipt_primary: &'a str,
    pub receipt_generation: &'a str,
    pub receipt_rustc_identity: &'a str,
    pub receipt_version: &'a str,
}

const KEY_SCRIPT_READ_ONLY: &str = r#"set -eu; set -o pipefail; case "$RUNNER_OS:$RUNNER_ARCH" in Linux:X64|Linux:x64|Linux:AMD64) os=linux; arch=x64 ;; Linux:ARM64|Linux:arm64) os=linux; arch=arm64 ;; *) echo "unsupported runner for MBX directory cache key" >&2; exit 1 ;; esac; test "$MBX_VERSION" = "$MBX_EXPECTED_VERSION"; test -n "$RUSTUP_TOOLCHAIN"; workflow_ref=${GITHUB_WORKFLOW_REF:?missing GITHUB_WORKFLOW_REF}; case "$workflow_ref" in *@refs/*) workflow_path=${workflow_ref%%@refs/*} ;; *) echo "invalid GITHUB_WORKFLOW_REF" >&2; exit 1 ;; esac; test -n "$workflow_path"; scope_file="$GITHUB_OUTPUT.mbx-scope"; identity_file="$GITHUB_OUTPUT.mbx-rustc"; hash_file="$GITHUB_OUTPUT.mbx-hash"; for file in "$scope_file" "$identity_file" "$hash_file"; do if [ -e "$file" ] || [ -L "$file" ]; then echo "MBX key marker already exists" >&2; exit 1; fi; done; printf '%s\n%s\n%s\n' "$workflow_path" "$MBX_CACHE_SCOPE" "$MBX_MATRIX_CONTEXT" > "$scope_file"; sha256sum "$scope_file" | cut -c1-64 > "$hash_file"; IFS= read -r scope_hash < "$hash_file"; test -n "$scope_hash"; mise --no-config --no-env --no-hooks exec "rust@$RUSTUP_TOOLCHAIN" -- rustc -vV > "$identity_file"; sha256sum "$identity_file" | cut -c1-64 > "$hash_file"; IFS= read -r compiler_hash < "$hash_file"; test -n "$compiler_hash"; toolchain="rust-${RUSTUP_TOOLCHAIN}-${compiler_hash}"; revision="$GITHUB_SHA"; if [ -n "$MBX_BASE_SHA" ]; then revision="$MBX_BASE_SHA"; fi; case "$revision" in ''|*[!0-9a-f]*) exit 1 ;; esac; test "${#revision}" -eq 40; prefix="${os}-${arch}-mbx-${MBX_GENERATION}-dir-${toolchain}-scope-${scope_hash}-";"#;

const KEY_SCRIPT_SAME_REPOSITORY: &str = r#"set -eu; set -o pipefail; case "$RUNNER_OS:$RUNNER_ARCH" in Linux:X64|Linux:x64|Linux:AMD64) os=linux; arch=x64 ;; Linux:ARM64|Linux:arm64) os=linux; arch=arm64 ;; *) echo "unsupported runner for MBX directory cache key" >&2; exit 1 ;; esac; test "$MBX_VERSION" = "$MBX_EXPECTED_VERSION"; test -n "$RUSTUP_TOOLCHAIN"; workflow_ref=${GITHUB_WORKFLOW_REF:?missing GITHUB_WORKFLOW_REF}; case "$workflow_ref" in *@refs/*) workflow_path=${workflow_ref%%@refs/*} ;; *) echo "invalid GITHUB_WORKFLOW_REF" >&2; exit 1 ;; esac; test -n "$workflow_path"; pr_cache_allowed=false; pr_namespace=; if [ "$MBX_PR_CACHE_POLICY" = "same-repository-scoped" ] && [ "$MBX_EVENT_NAME" = "pull_request" ] && [ "$MBX_HEAD_REPOSITORY_FORK" = "false" ] && [ -n "$MBX_REPOSITORY" ] && [ "$MBX_HEAD_REPOSITORY" = "$MBX_REPOSITORY" ] && [ -n "$MBX_BASE_REPOSITORY" ] && [ "$MBX_BASE_REPOSITORY" = "$MBX_REPOSITORY" ]; then case "$MBX_PR_NUMBER" in ''|*[!0-9]*) ;; *) case "$MBX_PR_NUMBER" in *[1-9]*) case "$MBX_PR_HEAD_SHA" in *[!0-9a-f]*) ;; *) if [ "${#MBX_PR_HEAD_SHA}" -eq 40 ]; then pr_cache_allowed=true; pr_namespace="same-repository-pr-${MBX_PR_NUMBER}-${MBX_PR_HEAD_SHA}"; fi ;; esac ;; esac ;; esac; fi; scope_file="$GITHUB_OUTPUT.mbx-scope"; identity_file="$GITHUB_OUTPUT.mbx-rustc"; hash_file="$GITHUB_OUTPUT.mbx-hash"; for file in "$scope_file" "$identity_file" "$hash_file"; do if [ -e "$file" ] || [ -L "$file" ]; then echo "MBX key marker already exists" >&2; exit 1; fi; done; if [ "$pr_cache_allowed" = true ]; then printf '%s\n%s\n%s\n%s\n' "$workflow_path" "$MBX_CACHE_SCOPE" "$MBX_MATRIX_CONTEXT" "$pr_namespace" > "$scope_file"; revision="$MBX_PR_HEAD_SHA"; else printf '%s\n%s\n%s\n' "$workflow_path" "$MBX_CACHE_SCOPE" "$MBX_MATRIX_CONTEXT" > "$scope_file"; revision="$GITHUB_SHA"; if [ -n "$MBX_BASE_SHA" ]; then revision="$MBX_BASE_SHA"; fi; fi; sha256sum "$scope_file" | cut -c1-64 > "$hash_file"; IFS= read -r scope_hash < "$hash_file"; test -n "$scope_hash"; mise --no-config --no-env --no-hooks exec "rust@$RUSTUP_TOOLCHAIN" -- rustc -vV > "$identity_file"; sha256sum "$identity_file" | cut -c1-64 > "$hash_file"; IFS= read -r compiler_hash < "$hash_file"; test -n "$compiler_hash"; toolchain="rust-${RUSTUP_TOOLCHAIN}-${compiler_hash}"; case "$revision" in ''|*[!0-9a-f]*) exit 1 ;; esac; test "${#revision}" -eq 40; prefix="${os}-${arch}-mbx-${MBX_GENERATION}-dir-${toolchain}-scope-${scope_hash}-"; printf 'pr-cache-allowed=%s\n' "$pr_cache_allowed" >> "$GITHUB_OUTPUT";"#;

const QUALIFICATION_KEY_SCRIPT: &str = r#"run_id=${GITHUB_RUN_ID:?missing GITHUB_RUN_ID}; run_attempt=${GITHUB_RUN_ATTEMPT:?missing GITHUB_RUN_ATTEMPT}; case "$run_id" in ''|*[!0-9]*) echo "invalid GITHUB_RUN_ID" >&2; exit 1 ;; esac; case "$run_attempt" in ''|*[!0-9]*) echo "invalid GITHUB_RUN_ATTEMPT" >&2; exit 1 ;; esac; prefix="${prefix}run-${run_id}-attempt-${run_attempt}-"; printf 'primary=%s%s\nprefix=%s\ngeneration=%s\nrustc_identity=%s\nmbx_version=%s\n' "$prefix" "$revision" "$prefix" "$MBX_GENERATION" "$compiler_hash" "$MBX_VERSION" >> "$GITHUB_OUTPUT""#;
const QUALIFICATION_OBSERVER_KEY_SCRIPT: &str = r#"run_id=${MBX_OBSERVER_CHILD_RUN_ID:?missing observer child run id}; run_attempt=${MBX_OBSERVER_CHILD_ATTEMPT:?missing observer child attempt}; for decimal in "$run_id" "$run_attempt"; do case "$decimal" in ''|0|0*|*[!0-9]*) echo "invalid observer run identity" >&2; exit 1 ;; esac; test "${#decimal}" -le 20; done; test "$GITHUB_EVENT_NAME" = workflow_dispatch; test "$GITHUB_REF" = refs/heads/main; test "$GITHUB_REF_PROTECTED" = true; test "$MBX_OBSERVER_ROLE" = reader; case "$MBX_CACHE_SCOPE" in qualification-mbx-v1/*) ;; *) echo "observer scope is outside the qualification namespace" >&2; exit 1 ;; esac; test "$revision" = "$MBX_OBSERVER_SOURCE_SHA"; case "$compiler_hash" in ''|*[!0-9a-f]*) echo "invalid measured rustc identity" >&2; exit 1 ;; esac; test "${#compiler_hash}" -eq 64; test "$MBX_GENERATION" = "$MBX_OBSERVER_RECEIPT_GENERATION"; test "$MBX_VERSION" = "$MBX_EXPECTED_VERSION"; test "$MBX_VERSION" = "$MBX_OBSERVER_RECEIPT_VERSION"; test "$compiler_hash" = "$MBX_OBSERVER_RECEIPT_RUSTC_IDENTITY"; prefix="${prefix}run-${run_id}-attempt-${run_attempt}-"; primary="${prefix}${revision}"; test "$primary" = "$MBX_OBSERVER_RECEIPT_PRIMARY"; printf 'primary=%s\nprefix=%s\ngeneration=%s\nrustc_identity=%s\nmbx_version=%s\n' "$primary" "$prefix" "$MBX_GENERATION" "$compiler_hash" "$MBX_VERSION" >> "$GITHUB_OUTPUT""#;
const KEY_OUTPUT_SCRIPT: &str = r#"printf 'primary=%s%s\nprefix=%s\ngeneration=%s\nrustc_identity=%s\nmbx_version=%s\n' "$prefix" "$revision" "$prefix" "$MBX_GENERATION" "$compiler_hash" "$MBX_VERSION" >> "$GITHUB_OUTPUT""#;

pub(super) fn key_step(
    name: &str,
    generation: &str,
    version: &str,
    scope: &str,
    rust_env: &BTreeMap<String, String>,
    qualification_nonce: bool,
    pull_request_cache_policy: PullRequestCachePolicy,
) -> Result<Step, RenderError> {
    let mut env = base_key_env(generation, version, scope, rust_env);
    if pull_request_cache_policy == PullRequestCachePolicy::SameRepositoryScoped {
        env.extend(BTreeMap::from([
            (
                "MBX_PR_CACHE_POLICY".to_owned(),
                "same-repository-scoped".to_owned(),
            ),
            (
                "MBX_EVENT_NAME".to_owned(),
                "${{ github.event_name }}".to_owned(),
            ),
            (
                "MBX_REPOSITORY".to_owned(),
                "${{ github.repository }}".to_owned(),
            ),
            (
                "MBX_HEAD_REPOSITORY".to_owned(),
                "${{ github.event.pull_request.head.repo.full_name }}".to_owned(),
            ),
            (
                "MBX_BASE_REPOSITORY".to_owned(),
                "${{ github.event.pull_request.base.repo.full_name }}".to_owned(),
            ),
            (
                "MBX_HEAD_REPOSITORY_FORK".to_owned(),
                "${{ toJSON(github.event.pull_request.head.repo.fork) }}".to_owned(),
            ),
            (
                "MBX_PR_NUMBER".to_owned(),
                "${{ github.event.pull_request.number }}".to_owned(),
            ),
            (
                "MBX_PR_HEAD_SHA".to_owned(),
                "${{ github.event.pull_request.head.sha }}".to_owned(),
            ),
        ]));
    }
    let output_script = if qualification_nonce {
        QUALIFICATION_KEY_SCRIPT
    } else {
        KEY_OUTPUT_SCRIPT
    };
    let key_script = if pull_request_cache_policy == PullRequestCachePolicy::ReadOnly {
        KEY_SCRIPT_READ_ONLY
    } else {
        KEY_SCRIPT_SAME_REPOSITORY
    };
    crate::steps::shell_step(
        name,
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            format!("{key_script}{output_script}"),
        ],
        env,
    )
}

pub(super) fn qualification_observer_key_step(
    name: &str,
    generation: &str,
    version: &str,
    scope: &str,
    rust_env: &BTreeMap<String, String>,
    binding: QualificationObserverBinding<'_>,
) -> Result<Step, RenderError> {
    let mut env = base_key_env(generation, version, scope, rust_env);
    env.extend(BTreeMap::from([
        ("MBX_BASE_SHA".to_owned(), binding.source_sha.to_owned()),
        ("MBX_OBSERVER_ROLE".to_owned(), "reader".to_owned()),
        (
            "MBX_OBSERVER_CHILD_RUN_ID".to_owned(),
            binding.child_run_id.to_owned(),
        ),
        (
            "MBX_OBSERVER_CHILD_ATTEMPT".to_owned(),
            binding.child_attempt.to_owned(),
        ),
        (
            "MBX_OBSERVER_SOURCE_SHA".to_owned(),
            binding.source_sha.to_owned(),
        ),
        (
            "MBX_OBSERVER_RECEIPT_PRIMARY".to_owned(),
            binding.receipt_primary.to_owned(),
        ),
        (
            "MBX_OBSERVER_RECEIPT_GENERATION".to_owned(),
            binding.receipt_generation.to_owned(),
        ),
        (
            "MBX_OBSERVER_RECEIPT_RUSTC_IDENTITY".to_owned(),
            binding.receipt_rustc_identity.to_owned(),
        ),
        (
            "MBX_OBSERVER_RECEIPT_VERSION".to_owned(),
            binding.receipt_version.to_owned(),
        ),
    ]));
    let mut step = crate::steps::shell_step(
        name,
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            format!("{KEY_SCRIPT_READ_ONLY}{QUALIFICATION_OBSERVER_KEY_SCRIPT}"),
        ],
        env,
    )?;
    step.condition = Some(QUALIFICATION_OBSERVER_CONDITION.to_owned());
    Ok(step)
}

fn base_key_env(
    generation: &str,
    version: &str,
    scope: &str,
    rust_env: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::from([
        (
            "MBX_VERSION".to_owned(),
            "${{ steps.mbx.outputs.mbx-version }}".to_owned(),
        ),
        ("MBX_EXPECTED_VERSION".to_owned(), version.to_owned()),
        ("MBX_GENERATION".to_owned(), generation.to_owned()),
        ("MBX_CACHE_SCOPE".to_owned(), scope.to_owned()),
        (
            "MBX_MATRIX_CONTEXT".to_owned(),
            "${{ toJSON(matrix) }}".to_owned(),
        ),
        (
            "MBX_BASE_SHA".to_owned(),
            "${{ github.event.pull_request.base.sha }}".to_owned(),
        ),
    ]);
    env.extend(rust_env.clone());
    env
}

pub(super) const QUALIFICATION_OBSERVER_CONDITION: &str = "success() && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true && steps.mbx-cancel-receipt.outputs.should_observe == 'true'";

pub(super) fn prep_condition(policy: PullRequestCachePolicy) -> String {
    if policy == PullRequestCachePolicy::ReadOnly {
        return "success() && github.event_name == 'push' && github.ref_name == github.event.repository.default_branch && steps.mbx-bundle.outputs.cache-hit != 'true'".to_owned();
    }
    format!(
        "success() && ({}) && steps.mbx-bundle.outputs.cache-hit != 'true'",
        writer_scope()
    )
}

pub(super) fn save_condition(policy: PullRequestCachePolicy) -> String {
    if policy == PullRequestCachePolicy::ReadOnly {
        return "success() && github.event_name == 'push' && github.ref_name == github.event.repository.default_branch && steps.mbx-bundle.outputs.cache-hit != 'true' && steps.mbx-export.outputs.ready == 'true'".to_owned();
    }
    format!(
        "success() && ({}) && steps.mbx-bundle.outputs.cache-hit != 'true' && steps.mbx-export.outputs.ready == 'true'",
        writer_scope()
    )
}

fn writer_scope() -> String {
    const TRUSTED_PUSH: &str =
        "github.event_name == 'push' && github.ref_name == github.event.repository.default_branch";
    format!(
        "({TRUSTED_PUSH}) || (github.event_name == 'pull_request' && {PR_CACHE_ALLOWED_OUTPUT} == 'true')"
    )
}

pub(super) fn rebind_lane_condition(
    condition: &mut String,
    policy: PullRequestCachePolicy,
) -> Result<(), RenderError> {
    let internal = "steps.mbx-bundle.outputs.cache-hit";
    if !condition.contains(internal) {
        return Err(RenderError::InvalidWorkflow(
            "mbx_shared_cache_hit_reference_missing".to_owned(),
        ));
    }
    *condition = condition.replace(internal, "steps.mbx-lane-cache.outputs.mbx-cache-hit");
    let internal = PR_CACHE_ALLOWED_OUTPUT;
    if policy == PullRequestCachePolicy::ReadOnly {
        if condition.contains(internal) {
            return Err(RenderError::InvalidWorkflow(
                "mbx_read_only_condition_has_pr_cache_output".to_owned(),
            ));
        }
        return Ok(());
    }
    if !condition.contains(internal) {
        return Err(RenderError::InvalidWorkflow(
            "mbx_shared_pr_cache_allowed_reference_missing".to_owned(),
        ));
    }
    *condition = condition.replace(
        internal,
        "steps.mbx-lane-cache.outputs.mbx-pr-cache-allowed",
    );
    Ok(())
}

#[cfg(test)]
#[path = "mbx_bundle_pr_cache_tests.rs"]
mod tests;
