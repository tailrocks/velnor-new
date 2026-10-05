//! Same-repository pull-request cache identity and write authorization.

use std::collections::BTreeMap;

use velnor_actions_contract::{PullRequestCachePolicy, Step};

use crate::RenderError;
pub(super) const PR_CACHE_ALLOWED_OUTPUT: &str = "steps.mbx-cache-key.outputs.pr-cache-allowed";

const KEY_SCRIPT_PREFIX: &str = r#"set -euo pipefail
case "$RUNNER_OS:$RUNNER_ARCH" in
  Linux:X64) os=linux; arch=x64 ;;
  Linux:ARM64) os=linux; arch=arm64 ;;
  macOS:X64) os=darwin; arch=x64 ;;
  macOS:ARM64) os=darwin; arch=arm64 ;;
  Windows:X64) os=win32; arch=x64 ;;
  Windows:ARM64) os=win32; arch=arm64 ;;
  *) printf "unsupported MBX runner %s/%s\n" "$RUNNER_OS" "$RUNNER_ARCH" >&2; exit 1 ;;
esac
rust_file="$RUNNER_TEMP/mbx-rustc-identity-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${GITHUB_JOB}"
digest_file="${rust_file}-sha256"
rustc "+$RUST_TOOLCHAIN" -vV > "$rust_file"
if [ "$os" = darwin ]; then
  shasum -a 256 "$rust_file" > "$digest_file"
else
  sha256sum "$rust_file" > "$digest_file"
fi
IFS=" " read -r identity _ < "$digest_file"
rm -f "$rust_file" "$digest_file"
identity="${identity:0:12}"
test -n "$identity"
toolchain="rust-${identity}"
"#;

const READ_ONLY_REVISION: &str = r#"revision="${CACHE_REVISION:?missing CACHE_REVISION}"
case "$revision" in ""|*[!0-9a-f]*) exit 1 ;; esac
test "${#revision}" -eq 40
"#;

const SAME_REPOSITORY_REVISION: &str = r#"pr_cache_allowed=false
revision="${CACHE_REVISION:-}"
if [ "$MBX_PR_CACHE_POLICY" = "same-repository-scoped" ] &&
  [ "$MBX_EVENT_NAME" = "pull_request" ] &&
  [ "$MBX_HEAD_REPOSITORY_FORK" = "false" ] &&
  [ -n "$MBX_REPOSITORY" ] &&
  [ "$MBX_HEAD_REPOSITORY" = "$MBX_REPOSITORY" ] &&
  [ "$MBX_BASE_REPOSITORY" = "$MBX_REPOSITORY" ]; then
  case "$MBX_PR_NUMBER" in
    ""|0*|*[!0-9]*) ;;
    *)
      case "$MBX_PR_HEAD_SHA" in
        *[!0-9a-f]*) ;;
        *)
          if [ "${#MBX_PR_HEAD_SHA}" -eq 40 ]; then
            pr_cache_allowed=true
            revision="same-repository-pr-${MBX_PR_NUMBER}-${MBX_PR_HEAD_SHA}"
          fi
          ;;
      esac
      ;;
  esac
fi
if [ "$pr_cache_allowed" = false ]; then
  case "$revision" in ""|*[!0-9a-f]*) exit 1 ;; esac
  test "${#revision}" -eq 40
fi
printf "pr-cache-allowed=%s\n" "$pr_cache_allowed" >> "$GITHUB_OUTPUT"
"#;

const KEY_SCRIPT_SUFFIX: &str = r#"key="${os}-${arch}-mbx-${CACHE_GENERATION}-dir-${toolchain}-${GITHUB_JOB}-${revision}"
case "$key" in ""|*-) exit 1 ;; esac
prefix="${key%-*}-"
printf "key=%s\nprefix=%s\n" "$key" "$prefix" >> "$GITHUB_OUTPUT"
if [ "$CREATE_EXPORT_GROUP" = true ]; then
  test -r /proc/sys/kernel/random/uuid
  IFS= read -r group_id < /proc/sys/kernel/random/uuid
  test -n "$group_id"
  group="github-actions-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-${group_id}"
  printf "MBX_CACHE_EXPORT_GROUP=%s\n" "$group" >> "$GITHUB_ENV"
fi
"#;

const TRUSTED_PUSH: &str = "github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true";
const TRUSTED_PUSH_NAME: &str = "github.event_name == 'push' && github.ref_name == github.event.repository.default_branch && github.ref_protected == true";

/// Build the one step that calculates and exports a job's MBX cache key.
///
/// # Errors
/// Returns an error if the generated shell step violates renderer contracts.
pub(super) fn key_step(
    action_sha: &str,
    mbx_version: &str,
    rust_toolchain: &str,
    create_export_group: bool,
    policy: PullRequestCachePolicy,
) -> Result<Step, RenderError> {
    let generation = format!(
        "{}-share-out-dir-disabled-v1-action-{action_sha}-dir",
        velnor_actions_contract::cachekey::mbx_cache_generation(mbx_version)
    );
    let mut env = key_environment(&generation, rust_toolchain, create_export_group);
    if policy == PullRequestCachePolicy::SameRepositoryScoped {
        env.extend(pr_identity_environment());
    }
    let script = key_script(policy);
    crate::steps::shell_step(
        super::MBX_CACHE_KEY_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), script],
        env,
    )
}

fn key_environment(
    generation: &str,
    rust_toolchain: &str,
    create_export_group: bool,
) -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "CREATE_EXPORT_GROUP".to_owned(),
            create_export_group.to_string(),
        ),
        ("CACHE_GENERATION".to_owned(), generation.to_owned()),
        (
            "CACHE_REVISION".to_owned(),
            "${{ github.event_name == 'pull_request' && github.event.pull_request.base.sha || github.sha }}".to_owned(),
        ),
        ("RUST_TOOLCHAIN".to_owned(), rust_toolchain.to_owned()),
    ])
}

fn pr_identity_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
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
    ])
}

fn key_script(policy: PullRequestCachePolicy) -> String {
    let revision = match policy {
        PullRequestCachePolicy::ReadOnly => READ_ONLY_REVISION,
        PullRequestCachePolicy::SameRepositoryScoped => SAME_REPOSITORY_REVISION,
    };
    let script = format!("{KEY_SCRIPT_PREFIX}{revision}{KEY_SCRIPT_SUFFIX}");
    let lines = script.lines().map(quote_double_literal).collect::<Vec<_>>();
    format!(
        "set -euo pipefail; printf \"%s\\n\" {} | bash",
        lines.join(" ")
    )
}

fn quote_double_literal(line: &str) -> String {
    let mut escaped = String::with_capacity(line.len());
    for character in line.chars() {
        if matches!(character, '$' | '`' | '"' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    format!("\"{escaped}\"")
}

#[cfg(test)]
#[path = "mbx_bundle_pr_cache_tests.rs"]
mod tests;

/// Select write mode only for protected pushes and explicitly admitted PRs.
pub(super) fn cache_mode_expression(policy: PullRequestCachePolicy) -> String {
    match policy {
        PullRequestCachePolicy::ReadOnly => {
            format!("${{{{ {TRUSTED_PUSH} && 'write' || 'read' }}}}")
        }
        PullRequestCachePolicy::SameRepositoryScoped => format!(
            "${{{{ (({TRUSTED_PUSH}) || (github.event_name == 'pull_request' && {PR_CACHE_ALLOWED_OUTPUT} == 'true')) && 'write' || 'read' }}}}"
        ),
    }
}

/// Authorize an export only on a trusted push or validated scoped PR.
pub(super) fn prep_condition(policy: PullRequestCachePolicy) -> String {
    if policy == PullRequestCachePolicy::ReadOnly {
        return format!(
            "success() && {TRUSTED_PUSH} && steps.mbx-bundle.outputs.cache-hit != 'true'"
        );
    }
    format!(
        "success() && ({}) && steps.mbx-bundle.outputs.cache-hit != 'true'",
        writer_scope(policy)
    )
}

/// Authorize a save only after an eligible export succeeded.
pub(super) fn save_condition(policy: PullRequestCachePolicy) -> String {
    if policy == PullRequestCachePolicy::ReadOnly {
        return format!(
            "success() && {TRUSTED_PUSH} && steps.mbx-bundle.outputs.cache-hit != 'true' && steps.mbx-export.outputs.ready == 'true'"
        );
    }
    format!(
        "success() && ({}) && steps.mbx-bundle.outputs.cache-hit != 'true' && steps.mbx-export.outputs.ready == 'true'",
        writer_scope(policy)
    )
}

fn writer_scope(policy: PullRequestCachePolicy) -> String {
    if policy == PullRequestCachePolicy::ReadOnly {
        TRUSTED_PUSH.to_owned()
    } else {
        format!(
            "({TRUSTED_PUSH_NAME}) || (github.event_name == 'pull_request' && {PR_CACHE_ALLOWED_OUTPUT} == 'true')"
        )
    }
}
