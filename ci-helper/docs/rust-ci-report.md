<!-- SPDX-FileCopyrightText: 2026 Alexey Zhokhov -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Rust CI report

`ci-metrics.json` is the per-lane, schema-1 report. The checker reads these
files from artifact directories and recomputes policy from their evidence;
`violations` is useful for display, but is not trusted as the only verdict.
The top-level report and policy structs reject unknown fields. Nested Cargo
artifact records tolerate additional upstream fields. Change the schema
deliberately when changing this contract.

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
  unavailable, or the sorted changed/created `.crate` paths relative to Cargo's
  registry cache. Each distinct changed archive contributes one download entry,
  even when Cargo prints no progress. Matching progress enriches its label;
  otherwise the registry-relative archive path is the label. Repeated progress
  and repeated archive paths count once. Equal filenames in different registry
  directories count separately.
- `verified_git_downloads` records changed Git source repositories, separately
  from registry crate downloads. Git object and reference observations are
  bounded; unavailable observations remain `null`. One repository may contain
  several crates, so the summary reports repository fetches and leaves their
  crate count unknown.
- `downloaded` and `slowest_tests` are aggregate display fields. The latter is
  capped at ten entries, longest first, equal times in name order. Every
  test job, including each test group job, reports its own list. `tests_run` is a display-only workflow value.
- `first_cargo_seconds` measures job start to the first Cargo invocation, or
  is `null` when the API cannot establish the job start time.
- `diagnostics` describe optional or incomplete telemetry. `verification_errors`
  mean evidence needed for a warm-cache decision could not be checked; the
  checker fails closed on these errors. `violations` are policy findings.

The checker validates artifact and observed-unit identity across lanes. Cargo
package identities must provide source, name, and version before their
artifacts can classify compilation. Source identifiers remain opaque;
structurally unsupported identities become verification errors.
Progress text alone cannot prove a download when a source cache was an exact
hit: registry archive and Git database observations are required, including
for quiet invocations. An
available empty observation proves zero downloads; an unavailable observation
cannot establish that result. See Cargo's
[JSON message format](https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages)
for the compiler-artifact and build-script records used as input evidence.

## Producer and enforcement

`VELNOR_REPORT_CACHES` supplies an array of cache plans. Each plan contains
only `family`, `root` (a Cargo root or `null`), `restore_step`, and `save_step`.
Observed report fields such as `requested_key`, `hit`, and `usable` are
rejected as plan input. `VELNOR_REPORT_STEPS` supplies the actual step outputs
and outcomes. The report resolves keys, restore hits, and save outcomes before
collecting restored input provenance, even when no Cargo invocation ran.
Malformed step contexts, missing planned restore steps, and unsuccessful or
invalid restore evidence retain the metrics artifact with verification errors.
An explicitly skipped restore and a successful restore with no matched key
remain known cold observations.
The inputs manifest, restored snapshot, and pre-Cargo witness live under
`.velnor/` in each root's configured target directory. The report resolves that
directory from the detected Cargo configuration. Inputs save and restore use
`--target-dir` relative to their Cargo working directory; tracked input scope
remains the Cargo root even when the target is outside that folder. Saving
after Cargo preserves both the restored snapshot and the pre-Cargo witness.
Target data is usable only when a restored snapshot exists and the restore
step reports a hit. A stale snapshot with a cache miss remains cold. Snapshot
parse or read errors make provenance unverifiable; a missing snapshot is cold.
`restored_target_usable` applies that provenance to each invocation's effective
target directory. A command overriding the configured cached target remains
cold for target-cache rules and retains its compilation counts. Exact source
cache checks still apply to that command.
A tracked `Cargo.lock` symlink prevents same-lock verification for its Cargo
root. A usable target with that symlink makes compilation provenance
unverifiable, including quiet non-fresh artifacts. Cold runs still retain
compilation evidence. Fresh artifacts alone do not claim compilation.
Other tracked symlink inputs also make warm compilation provenance
unverifiable. Input snapshots store link text, without historical referent
contents or directory identities. Even a link whose current target is tracked
cannot prove that the restored build read the same contents. The report does
not reconstruct historical referents from the current checkout.
Current or saved symlink inputs disable an unchanged-workspace claim within
the affected root and leave independent roots verifiable. Cold runs retain
compilation counts without a provenance error. Fresh artifacts alone do not
require compilation provenance.
Repeated compilations without a recognized unit identity retain their observed
package and available configuration context. Three such observations in one
context make the check fail verification, because it cannot prove that Cargo
respected the compilation limit. This is uncertainty rather than proof that
one identical unit compiled three times.
Named target selectors and command purpose distinguish fallback observations,
so separate test or binary targets do not become repeated compilations of one
unit. Unhashed documentation outputs cannot establish compiler identity and
remain unknown, like ordinary unhashed binary outputs.
API enrichment runs after provenance collection. These inputs do not change
the schema-1 output contract.

Generated report and upload steps continue on error. The separate `Cache`
job reports missing or invalid metrics. API lookups are read-only. Job timing
comes from the current workflow run; cache sizes come from eligible cache
entries in the current repository. Fork pull-request report steps receive an
empty explicit API token. Unavailable API fields remain unknown and produce
diagnostics; they are not reported as zero-byte archives or successful saves.

Generated reporting requires helper `0.1.81` or a supported source-built
helper. The `Cache` job fails on waste or unverifiable required evidence.

`velnor-ci report --check <artifacts>` requires metrics for every non-skipped
lane named by `VELNOR_REPORT_NEEDS`, validates schema and evidence, and
recomputes cache waste. Skipped lanes need no artifact. A successful report
generation alone does not imply that the cross-lane check passed.
