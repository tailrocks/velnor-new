pub(crate) const RETRY_DELAYS_SECONDS: [u16; 5] = [3, 6, 12, 24, 45];

pub(super) const EMPTY_LISTING_JQ: &str = "[.[] | (.workflow_runs // [])[]] | length == 0";

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
