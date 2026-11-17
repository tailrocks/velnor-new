<!-- SPDX-FileCopyrightText: 2026 Alexey Zhokhov -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Rust CI report

`ci-metrics.json` is the per-lane, schema-1 report. The checker reads these
files from artifact directories and recomputes policy from their evidence;
`violations` is useful for display, but is not trusted as the only verdict.
The JSON structs reject unknown fields. Change the schema deliberately when
changing this contract.

## Lane fields

- `lane` identifies the job; `caches` describes each cache family and root.
- Cache keys include `requested_key` and `matched_key`; `hit` is `exact`,
  `fallback`, or `miss`. `bytes` is the archive byte count, or `null` when the
  API cannot establish it. `usable` and `usable_same_lock` describe restored
  target data. `save` is `not_attempted`, `saved`, `failed`, or `unknown`.
- `invocations` records Cargo `argv`, root, normalized configuration, start
  time, elapsed seconds, output offsets, compilation evidence, downloads,
  compiler artifacts, progress, test timings, capture status, and execution
  context. `verified_download_archives` is `null` when archive observation was
  unavailable, or the sorted changed/created `.crate` basenames observed for
  that invocation.
- `downloaded` and `slowest_tests` are aggregate display fields. The latter is
  capped at ten entries. `tests_run` is a display-only workflow value.
- `diagnostics` describe optional or incomplete telemetry. `verification_errors`
  mean evidence needed for a warm-cache decision could not be checked; the
  checker fails closed on these errors. `violations` are policy findings.

The checker validates artifact and observed-unit identity across lanes. Cargo
package identities must be understood before their artifacts can be used to
classify compilation; unsupported identities become verification errors.
Progress text alone cannot prove a download when a source cache was an exact
hit: a matching archive observation is required. See Cargo's
[JSON message format](https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages)
for the compiler-artifact and build-script records used as input evidence.

## Producer and enforcement

The report producer is nonfatal so a missing GitHub API token or unavailable
API does not replace the job's build result with a telemetry failure. API
lookups are read-only and scoped to the current repository and workflow run.
Unavailable API fields remain unknown and produce diagnostics; they are not
reported as zero-byte archives or successful cache saves.

The `0.1.81` helper capability gate controls generated cache-save and
PR-only-check behavior. Cache reporting is descriptive unless a workflow opts
into blocking on the cache verdict. `strictCache` makes an opted-in cache
decision fail when the required cache evidence is unknown; it does not turn
optional telemetry into a required producer step.

`velnor-ci report --check <artifacts>` requires metrics for every non-skipped
lane named by `VELNOR_REPORT_NEEDS`, validates schema and evidence, and
recomputes cache waste. Skipped lanes need no artifact. A successful report
generation alone does not imply that the cross-lane check passed.
