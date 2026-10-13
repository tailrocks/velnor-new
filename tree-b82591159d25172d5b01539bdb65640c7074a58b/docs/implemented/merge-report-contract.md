# Merge report behavior contract (G3)

How `merge-v1` concludes when reports are missing, forged, or delayed.
Every claim below is HEAD behavior with its enforcing mechanism; the
merge cross-checks `needs` in-repo (contra "unverifiable from the repo").

Correction to the finding text: at HEAD only the plan download carries
`continue-on-error` (`document.rs:281-283`, gated by
`is_verdict_download`). Crate reports arrive through the fan-in fetch op
(`final_steps.rs:39-40`), which has bounded per-leg retry and no
`continue-on-error` (`document.rs:342-345`, F5).

## Missing plan report → `planning_failed` + `source_missing`

Absent `plan.json` records `missing_plan` at assembly
(`merge_request.rs:273-277`); the merge emits a diagnostic verdict
instead of an error (`merge.rs:106-112`), mapping every `missing_*`
detail to `source_missing` (`required_evidence.rs:222-235`).
`continue-on-error` on the download exists precisely so the failure
reaches the merge verdict instead of dying at the step
(`document.rs:204-213`).

## Missing crate report → `planning_failed` + `source_missing` / `not_run`

Persistent per-leg fetch failure skips that job's entries
(`retrieve_reports.rs:93-99`); unreadable files record
`missing_report:{artifact}` (`retrieve_reports.rs:289-294`), which maps
to `source_missing` and sets `planning_failed`
(`required_evidence.rs:104-108`). Entries with no valid report
additionally set `not_run` (`merge.rs:190-195`). Precedence keeps the
verdict `planning_failed` (`merge.rs:255-271`).

## Forged / tampered report → fail closed, no silent pass

There is no signature or HMAC on reports; authentication is binding to
the plan within the same run, enforced by exact checks:

- Matrix bytes must agree with the plan matrix, else `cache_corrupt`
  (`merge_checks.rs:22-39`).
- Per-task files outside the plan-derived expectation corrupt the set
  (`merge_checks.rs:346-350`); malformed files are `not_run`
  (`merge_checks.rs:334-339`).
- Task-report IDs re-derive from plan obligation digests; digest or
  identity mismatch fails the entry (`cover.rs:237-305`).
- Wrong-run files fail with `trust_scope_mismatch`
  (`merge_checks.rs:328-333`).
- The plan's event/trust must equal the merge job's own event ground
  truth, so a forged plan claiming stronger trust fails closed
  (`merge_checks.rs:62-85`).
- Reads reject symlinks and enforce size bounds (`merge_request.rs:257-292`,
  `retrieve_reports.rs:343-350`).

Weaker-than-fail-closed case: none found at HEAD for these checks; every
listed path sets `planning_failed`, `failed`, or `not_run`, never `passed`.

## Transient download failure → retried, then judged honestly

Each leg retries up to 3 attempts (`retrieve_retry.rs:13`); the fetch
still exits success on per-leg failure so the merge judges the gap
(`retrieve_reports.rs:9-13`, `document.rs:342-345`). Outcome is the
missing-crate-report case above, never silent success. Only an unusable
environment (no runner temp, no numeric run ID) fails outright
(`retrieve_reports.rs:44-60`).

## `needs` cross-check (the "unverifiable" step)

Merge assembly parses the `VELNOR_NEEDS_JSON` conclusions plus the
rendered `VELNOR_NEEDS_EXPECTED` inventory (`merge_request.rs:112`) and
fails closed on any observed-vs-expected divergence
(`needs_channel.rs:81-83`, `needs_inventory_mismatch`). The merge
re-enforces the exact set: empty inventory, duplicates, extras, and gaps
all fail closed (`required_evidence.rs:110-145`); conclusions fold with
skipped as `not_run` and missing as `failed`
(`required_evidence.rs:171-180`), and the final report mirrors the full
inventory with `missing` markers (`required_evidence.rs:186-198`).
