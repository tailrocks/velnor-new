# Freshness and tool qualification evidence — 2026-10-10

This record refreshes the version inventory using observed upstream responses
and records the exact-host qualification of Mise `2026.10.6`. The hosted
qualification and upstream probe below are bound to implementation source
`bbf2cb2df8380b173a0629d07cb862f4870588f8` (tree
`ebb160d5cbea5d021aac14c269f9e52aee86fda4`). The terminal local gate run is
bound to PR #126 code/evidence head `3538b248cbd0938ff73f645d87dcf29196a94aa1`
(tree `83799865fe6abddc6a957f2efd7b3344baa0488b`); both are based on main
`3139334cb79c0b494eb530b1de88ff258af10f21`. The PR is not merged. This record
does not claim Velnor `v0.1.6` publication, release qualification, or consumer
adoption.

## Mise `2026.10.6` hosted qualification

Qualification run [37995746931](https://github.com/tailrocks/velnor-new/actions/runs/37995746931)
was dispatched from the exact source commit above and completed successfully
at `2026-10-09T21:50:11Z`. Its two required jobs verified the version and
platform-specific executable digest:

| Target | Result | Evidence |
| --- | --- | --- |
| Linux x64, `ubuntu-26.04` | Passed | [job 114041252080](https://github.com/tailrocks/velnor-new/actions/runs/37995746931/job/114041252080) |
| macOS x64, `macos-15-intel` | Passed | [job 114041252169](https://github.com/tailrocks/velnor-new/actions/runs/37995746931/job/114041252169) |

The Linux log records Ubuntu `26.04.1`, runner image `ubuntu-26.04`, and image
version `20260927.149.1`. The run qualifies this Mise binary on those two
targets only. It is not generator-release qualification.

## Runner labels and observed image

The GitHub-hosted-runner reference was fetched at `2026-10-09T22:01:23Z`; its
public standard-runner table listed `ubuntu-26.04`, `ubuntu-24.04`, and
`ubuntu-22.04`, matching the policy and inventory labels. The saved reference
is `/private/tmp/velnor-freshness-pr126-1791582885318/github-hosted-runners-docs.md`
(SHA-256 `a9ec7930b95280432f5687584bafa19c42fd72c25c08a1fc43e6f86b0a3faf19`).
A query of 11 workflow runs from `2026-10-08T22:03:30Z` found no successful
target jobs on the `ubuntu-24.04` or `ubuntu-22.04` labels (saved query SHA-256
`d42096c5684a9f0570bd0bd4d37f604373eb09761fab9e931d01272026ebc2ce`). The
supported-label list remains unchanged because the policy tracks GitHub's
supported labels; this is not a Velnor qualification claim for `24.04` or
`22.04`.

## Upstream version observations

The canonical `scripts/check-freshness.sh --check-upstream` command on the
source above emitted 19 tool/action observations at `2026-10-09T22:40:38Z`:
14 matched their configured pin and five were newer than a pin already covered
by an unexpired hold. The command exited 1 because it also evaluated the then
stale/pending inventory; that exit is not recorded as a passing freshness
check. The exact 19 emitted rows, command context, and raw log binding are
preserved in [the probe receipt](freshness-upstream-probe-2026-10-09.json).
The raw log is retained at
`/private/tmp/velnor-pr126-gates-bbf2-fixedcontext-20261010/freshness-upstream-refresh.log`
(SHA-256 `b67989c10464445cff9b5b22958427f4629378117307959b7888ca95e9404193`).

The five newer values are Rust `1.99.0`, MBX `v1.23.0`,
`actions/download-artifact` `v8.0.2`, `actions/upload-artifact` `v7.0.2`, and
`jdx/mr-boxington-action` `v1.7.1`. Existing pins and hold versions remain
unchanged. The MBX tool and action holds retain owner, issue reference, grant
date, and expiry. PR #29 is closed unmerged and is cited only as historical
context; the current exact MBX `v1.23.0` source at
`fb614644ff98c8ee3a3f28a6d31b3ba0af37b17f` and action `v1.7.1` source at
`d0825fbaf3cc36ca2609aa38e71046265a1f1e37` still lack the required
protected-main cache lifecycle/disk/input qualification. Their holds were not
renewed or extended by this observation. Issue #6 remains open for its Rust
and artifact-action records.

The probe's latest-release API receipts captured MBX at
`2026-10-09T22:06:19Z` and its release commit at `22:06:20Z`; the action release
was observed at `22:06:24Z` and its commit at `22:06:25Z`. Their source file
hashes and exact identities are recorded in the JSON receipt. The inventory's
`latest` fields and `checked_at` values come from the canonical probe rows;
they are not timestamp-only renewals.

## Local verification status

The earlier full batch on implementation source `bbf2cb2` passed five of the
six standalone gate categories: formatting, workspace Clippy, workspace
Nextest, Alint, and Cargo Deny. Nextest reported 3,648 passed, zero failed,
and two skipped. The freshness gate failed on the then-pending Mise status and
stale observation timestamps; `scripts/verify-local.sh` exited 1 at
`repo-policy`. Those earlier logs are at
`/private/tmp/velnor-pr126-gates-bbf2-fixedcontext-20261010/` (`verify_local.log`
SHA-256 `a2b9b3aca0c16560bb41eefdb2080990926cef3e6faebd9a541b708624e3668c`).

The terminal full gate batch was run on clean code/evidence head
`3538b248cbd0938ff73f645d87dcf29196a94aa1`, tree
`83799865fe6abddc6a957f2efd7b3344baa0488b`, using the pinned tools recorded by
the batch script. All six standalone gate categories passed: formatting,
workspace Clippy, workspace Nextest (3,648 passed, zero failed, two skipped),
Alint validation and check, Cargo Deny, and freshness. The standalone Nextest
log passed `atomic_commit_never_exposes_missing_tree` at test 2,003 of 3,648.

The subsequent `scripts/verify-local.sh` invocation on that same code head
exited 1 at its integration stage. It ran 2,019 of 3,648 tests: 2,018 passed,
one failed, and two were skipped; Nextest reports that the remaining 1,629
tests were not run after the failure. The failed test was
`impl_generate_p09_atomic::atomic_commit_never_exposes_missing_tree`. Its
strict `.github` metadata observer recorded two `NotFound` errors with raw
errno 2 during `rewrite_4_in_progress`; it recorded zero other metadata
errors. The post-error probes found the parent and `.github` target present,
which does not establish why the earlier lookups failed. The cause remains
unresolved. The standalone Nextest pass and this later `verify-local` failure
are separate results; the pass does not clear the failure. PR #126 therefore
has no local merge or release clearance.

The final batch receipts are retained at
`/private/tmp/velnor-pr126-gates-3538-final-20261010/` with the adjacent
`velnor-pr126-gates-3538-final-20261010-run-gates.sh`. The script SHA-256 is
`b4f2acc9581f085150b3f87cdff55f7391bbc39c811c0565276fd0de4a7c1105`; the
standalone results table SHA-256 is
`b95d8903b9e433f9671019044b9c1556816f24fde8597f5496c1a5ce835ecb92`; the
standalone Nextest log SHA-256 is
`9fe6efc8ef7b091d687fda8b375d0571a9e7ff910345d96368c8fc6e8f8b6dea`; the
terminal `verify-local` summary SHA-256 is
`2623d0b905331840de7ec878ca7dc986a1975f5203afcc5b43bdcdafa69b7f3c`; and its
integration log SHA-256 is
`171d85d53ee51b4c76c827d2d9cc09414e9ee28aa303fc80d30d3bb8dc1fb09f`. An
older copied `verify-local-stage-logs/verify-local.log` in that directory is
dated `2026-10-05` and is not part of this terminal batch.

After the inventory and evidence update in the isolated evidence worktree,
`bash scripts/check-freshness.sh`, `alint validate-config`,
`alint check --fail-on-warning`, and `git diff --check` all exited 0. The
focused result table is at
`/private/tmp/velnor-refresh-qualified-tool-evidence-checks-20261010/results.tsv`;
the offline freshness log SHA-256 is
`962d4ae15f9e80e9a55fcf00cdb6fa9f691bd06c880709483ec4871f2c61e924`. Those
focused checks do not change the terminal full-gate result above.
