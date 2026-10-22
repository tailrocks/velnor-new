//! Exact-source CI run listing shared by every release gate.
//!
//! GitHub's workflow-runs listing intermittently answers an exact `head_sha`
//! query with no runs at all, although the run exists and has succeeded. On
//! 2026-10-22 thirty probes per form against a successful velnor-new main run
//! returned an empty listing 5 times with only `head_sha`, 8 times with
//! `head_sha` and `branch`, and 9 times with `head_sha`, `branch` and `event`.
//! Five product-release dispatches failed on it.
//!
//! Every gate therefore lists the runs of one source SHA with the `head_sha`
//! filter alone and keeps every identity check (workflow path, event, head
//! branch, head SHA, head repository) in jq, unchanged. The listing is
//! repeated, with backoff, only while it carries no run at all. Any listing
//! that carries a run is final, including one whose runs all fail an identity
//! check: a mismatch is never retried. An API or parse error fails at once,
//! and a listing that stays empty fails closed after the last attempt.

/// Seconds to wait before each repeated listing: six listings within 90 seconds.
pub(super) const RETRY_DELAYS_SECONDS: [u16; 5] = [3, 6, 12, 24, 45];

/// jq program that succeeds only when no page of a slurped listing carries a run.
pub(super) const EMPTY_LISTING_JQ: &str = "[.[] | (.workflow_runs // [])[]] | length == 0";

/// Bash function `list_exact_source_ci_runs LABEL COMMAND...`.
///
/// It runs COMMAND (a paginated, slurped workflow-runs listing) and prints its
/// output. It repeats COMMAND after each delay in [`RETRY_DELAYS_SECONDS`]
/// only while the listing carries no run. A failing COMMAND returns 1 at once
/// without a retry. Output that is not JSON, or that carries any run, is
/// printed as is for the caller's identity checks.
pub(super) fn bash_function() -> String {
    let delays = RETRY_DELAYS_SECONDS
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let attempts = RETRY_DELAYS_SECONDS.len() + 1;
    format!(
        r#"list_exact_source_ci_runs() {{
  local _velnor_label="$1"
  shift
  local _velnor_listing _velnor_delay
  for _velnor_delay in {delays} ''; do
    _velnor_listing="$("$@")" || return 1
    if ! printf '%s\n' "$_velnor_listing" | jq -e '{EMPTY_LISTING_JQ}' >/dev/null 2>&1; then
      printf '%s\n' "$_velnor_listing"
      return 0
    fi
    [[ -n "$_velnor_delay" ]] || break
    printf '%s: the CI run listing is empty; listing again in %ss\n' "$_velnor_label" "$_velnor_delay" >&2
    sleep "$_velnor_delay"
  done
  printf '%s: the CI run listing stayed empty after {attempts} attempts\n' "$_velnor_label" >&2
  printf '%s\n' "$_velnor_listing"
}}"#
    )
}

#[cfg(test)]
#[path = "schema2_ci_run_lookup_tests.rs"]
mod tests;
