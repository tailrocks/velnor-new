# Source-bound progress at 13c1419

Status: one committed inventory correction, with bounded local execution proof.
All47 performance statuses remain **INCOMPLETE** in the
[completion ledger](ci-performance-completion-ledger.md); C01–C09 and T01–T26
remain open. No full workspace, hosted runtime, cross-run reuse, consumer rollout
or performance qualification follows from this supplement.

## Pushed correction

Commit `13c1419718f540ab820e1c19336402d7619e1e82`, parent
`63e58d2d0c0e8440c680b1cba347a0d342a99af0`, changes only
`crates/velnor-actions-mise/tests/impl_mise_forbidden_src.rs`:
one insertion, `command_git.rs` after `command_env.rs` in `expected_modules()`.
The committed source inventory contained 41 modules; the expected inventory
contained 40. The exact source registration assertion exposed the missing entry.
The correction retains that assertion, source scanning and banned-token policy.
`git.rs` was already registered; neither product source file changed.

Captured commands record `git commit -s`, the DCO signoff and
`Co-authored-by: Codex <codex@openai.com>`, successful push to
`perf/cache-selection-qualification`, and matching remote-tracking head.
This records DCO signoff; it makes no cryptographic signature claim.
The parent receipt records preservation of concurrent physical future source.
That future source is outside the isolated proof below.

| Exact correction identity | Value |
|---|---|
| Test postimage Git blob | `02fbc590e2dedaa1a946924bd149fa4859b3984e` |
| Test postimage SHA256 | `8f1ca0bc8de92a0d76ae0d38531cd83100e6a775b565a947bfac441e3c06c61b` |
| One-hunk patch SHA256 | `e1e630d3c17080e1220d43f56feca65ca8dbd45480969c61fc91f6e75e4bc42c` |
| Exact 41-module inventory SHA256 | `3c0fc41b7cb6e4dffa3b4e50a0de554c2993b488c3220477464c68dd7c5bfdfa` |

Independent reviewer `committed_mise_allowlist_review` approved those exact
postimage/patch bytes and checked the source41/expected40 difference was solely
`command_git.rs`; future modules were not registered.

## Actual isolated proof

The execution owner archived exact parent `63e58d2…` into a separate source root,
applied the approved one-entry patch, and used absolute Rust 1.98.1 Cargo on
macOS arm64. The source manifest binds the complete tested file inventory and
patch. The isolated home, temporary directory and target directory were explicit;
the Cargo home reused an existing retained cache. This was local verification,
not a fresh hosted-runner or cold-cache experiment.

| Actual command, using that absolute Cargo | Recorded outcome |
|---|---|
| `cargo fmt --all -- --check` | Exit 0 |
| `cargo test --locked -p velnor-actions-mise --test velnor_mise impl_mise_forbidden_src:: -- --list` | Exit 0; exactly one compiled test selected |
| `cargo test --locked -p velnor-actions-mise --test velnor_mise impl_mise_forbidden_src:: -- --test-threads=1` | Exit 0; 1 passed, 0 failed, 0 ignored, 339 filtered out |

Executed test identity:
`impl_mise_forbidden_src::mise_sources_stay_read_only_and_unmanaged`.
The final proof records `root_unchanged=true`; the parent receipt joins the tested
postimage to the committed blob. These results establish the exact focused
correction. Full Clippy, workspace Nextest, Alint, cargo-deny, freshness and hosted
Required/default-branch checks are not established by this isolated execution.

Private records retained by filename and SHA256:

| Private evidence locator | SHA256 |
|---|---|
| `velnor-parent-allowlist-commit/receipt.json` | `b920c0a8619b5250e17ce1a282346a94ce4e01869b5e6796d868168a52483b27` |
| `velnor-head63e58-allowlist-fix/manifest-v2.json` | `32521cff93d603cc553c02e3d4f6392708dacb12aabb29d4f6f2757712bdd652` |
| `velnor-allowlist-runner-63e58/run/final-proof.json` | `7b8550b90cbec75a0b5af62be452bbcd063ee642561092513ee6c41c4860cf12` |
| `velnor-allowlist-runner-63e58/run/source.json` | `170bda41ec250fabf7acad59c7ad7f8d25c23dde3d792e53153a5dcb73efe7f4` |

The source record's inner canonical files-manifest SHA256 is
`d7c13193d247cb55d889d8a66e7f71495f2be27075f4344f9647a1dd07f482b4`;
the final execution proof retains that inner identity separately from file bytes.

## Actual hosted attempt at 13c1419

[PR12 run 37106672545](https://github.com/tailrocks/velnor-new/actions/runs/37106672545),
attempt 1, completed **failure** at head `13c1419…`. Actual integration checkout
was `63b3904cd00e333c6d6ae9cc6bd698ec16b0f0ac`, with base
`c57c700459bbe1549fe7eedcb7d8689585c38986`.

| Job | Actual result |
|---|---|
| [Mise 111156668619](https://github.com/tailrocks/velnor-new/actions/runs/37106672545/job/111156668619) | PASS; 339 passed, 1 skipped; suite 2.585 s |
| [Orchestrator 111156668661](https://github.com/tailrocks/velnor-new/actions/runs/37106672545/job/111156668661) | PASS; 1061 passed; suite 79.951 s |
| [CLI 111156668517](https://github.com/tailrocks/velnor-new/actions/runs/37106672545/job/111156668517) | FAIL; 17 stale freshness entries in `impl_cli_verify_local::verify_local_repo_policy_stage_executes`; 78 passed, 1 failed, 153 unrun |
| [Required 111157198139](https://github.com/tailrocks/velnor-new/actions/runs/37106672545/job/111157198139) | FAIL at Merge reports |
| [Baseline 111157255004](https://github.com/tailrocks/velnor-new/actions/runs/37106672545/job/111157255004) | Skipped; no allocated runner |

The observed Plan → orchestrator → Required interval is 312 s from run creation
through Required completion, including queue; inter-job waits are 13 and 2 s.
This failed-run interval and suite timings are unpaired observations from distinct
source/runner contexts. They establish no paired speedup, p95 or warm-budget
qualification. Separate link time, CPU features/count and actual hosted compiler
optimization flags remain unavailable in the nonverbose logs.

Private `velnor-pr12/13c1419/receipt.json` has SHA256
`0a77d578e068fa45d3a3d2f91521e9345be07e0aeee1b2c823c4909a1322d579`.
It binds 28 raw files; independent rehash here found zero mismatches.
The raw ZIP SHA256 is
`a405afeee19db364f8807740dd38b14697ba0a277283535e359ca37ee95b69bc`.
Captured feedback at this head contained zero reviews/comments/threads as of
`2026-10-03T08:43:16.296299+00:00`; this historical observation approves no later
head or merge. No raw private source or logs are reproduced in this supplement.

## Historical failures and remaining qualification

The independent correction review retains the earlier Mise result:
120 passed, 1 failed, 1 skipped and 218 unrun. Its frozen CI ZIP has SHA256
`ad07b002bbb10bc6b30a7f2d0bf953306902d8a01660bdd531f064d707f107c5`.
That historical failure is preserved; the later focused one-test pass does not
replace its whole-suite outcome or supply a new hosted result.

The earlier [b0df progress supplement](ci-performance-source-progress-b0df.md)
retains PR12 run `37099764713`: CLI failed on 17 stale freshness entries,
Required failed and baseline publication was skipped. Its successful individual
jobs and source publications remain bounded evidence. This correction does not
relabel that attempt or qualify the final generator/runtime.

Final reviewed source integration, all repository gates, actual hosted native and
generator runtime proof, controlled unchanged/changed/negative experiments and
consumer waves remain required. Unknown telemetry remains unknown. All47
performance statuses remain **INCOMPLETE**.
