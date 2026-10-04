# macOS Scale Set checklist

Status vocabulary: `PASS`, `FAIL`, `BLOCKED_EXTERNAL`, `NOT_RUN`. A skipped
test is not a pass. No row below is `PASS` unless `evidence.md` cites a command
this repository ran.

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| G0-identity | Current heads, tags, open PRs recorded | PASS | `evidence.md` identity table, parent `gh api` on 2026-10-03 |
| G0-consumer | ChainArgos tasks and required contexts inventoried | PASS | `evidence.md` coverage table |
| G0-supersede | Deferred conflicts marked superseded before feature code | PASS | `docs/deferred/self-hosted-runner.md` banners; `docs/proposed/macos-scaleset-runner.md` |
| G0-tree | Unrelated dirty work preserved | PASS | Only this branch's docs and later commits; goal prompt files stay untracked |
| G1-migrate | Schema migrate round-trip, idempotence, unknown fields | PASS | `impl_schema2` at `40e08e8`; preview does not write |
| G1-labels | Illegal hosted label on scale-set selector fails | PASS | `illegal_scale_set_label_rejected` |
| G1-capacity | Occupancy never exceeds N; no release without cleanup proof | PASS | `invariants.rs` occupancy and `CleanupProof` |
| G1-wire | Null/omit, message 0, empty poll, partial acquire, single-flight refresh | PASS | `velnor-runner-github` tests at `da68f44` |
| G2-journal | Crash before and after each external step; reopen real database | PASS | `around_commits_pending_before_effect_and_hides_secret` sees pending before the effect returns; secret bytes are absent from the database |
| G2-uncertain | Uncertain acquire and delete keep capacity | PASS | `release_permitted_gates_capacity_release` keeps occupancy at 1 until `CleanupProof`; `before_advertise_holds_uncertain_and_adopts` |
| G3-isolation | Private DinD, no outer socket/home, canary absent from metadata | PASS | Secret job `111162832876` masked the canary. Composite job `111165489828` shared `_actions` inode `128829173`, then removed only owned ids. |
| G3-ownership | Foreign objects survive; id mismatch quarantines | PASS | Foreign id `d377a7f7cfc3` survived `KeepForeign`. Job `111165489828` cleanup left the pre-existing 24 containers. |
| G4-live | Real GitHub job on official runner and scale-set labels | PASS | job `111084145716` runner `m100000009`; one-class jobs in `evidence.md`, including cache rerun `37093907324` and same-port run `37096417428`; full G4 suite still open |
| G5-routing | Schema 1 unchanged; `both` duplicates verification only | PASS | `impl_schema2_routing.rs` at `40e08e8`; goldens unchanged |
| G5-compare | Duplicate, missing lane, swapped artifact, unsafe archive fail closed | PASS | `compare_tests.rs` at `658154c` |
| G6-launchd | User LaunchAgent foreground `daemon run`; second daemon fails | PASS | `launchctl print gui/501` at 2026-10-03: absolute `daemon run`, `forks = 0`; second `daemon run` exit 1; bootout removed the job |
| G6-binary | `velnor-host` help and not-ready status | PASS | `help_exits_success_and_a_bad_command_does_not`; status JSON `waiting_for_credentials` |
| G7-publish | Published generator, image, and macOS binary consumed by ChainArgos | NOT_RUN | `generator-47815c83` was published and `baa78037` regenerated the consumer tree. Paired suite is not successful. The running host is not a published asset. |
| G7-paired | Hosted baseline, N=1 canary, N=2, cold and warm paired runs | NOT_RUN | Nc cold and warm cache/cancel/cancel-service on `sha256:7aaac3c4` are terminal and are not the suite. Warm cancel `37195872352` and cancel-service `37195876085` cancelled both lanes during the scale-set sleep. Warm cache `37195879504` restored `g4-cache-37195879504`. Class wave compose `37196069739` through secret `37196131679` is in flight. Not a pass. |
| G8-merge | Final main uses pinned published generator; required checks kept | NOT_RUN | no promotion |

Recovery gates use R0–R8. They do not renumber G0–G8.

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| R0 | Current source, product, run, and open-PR inventory | PASS | `evidence.md` recovery inventory, 2026-10-03 |
| R1 | Hosted MBX write then restore, bounded disk | FAIL | ENOSPC on `37114238559`. Push run `37163556069` wrote one bundle after df. That write is not the accepted restore, so this row stays FAIL. |
| R2 | Archive semantics on the deployed amd64 image | NOT_RUN | On `sha256:7aaac3c4`, `37195602237` restored `g4-cache-37195602237` and `37195879504` restored `g4-cache-37195879504` after `tar --posix`. Archive security regressions beyond that key are not proven. Not a pass. |
| R3 | Daemon lifecycle, N>1 backfill, cleanup | FAIL | Live pid 35645 is cdhash `c7e59739` (sha256 `788f363c`), not `de147432d`. Id-less rows 710, 717, 1005, and 1008 were marked `failed` so mint could resume. A second mint after exit is not proven. |
| R4 | Published repaired generator and full regenerated tree | NOT_RUN | `generator-47815c83` was published and ChainArgos `baa78037` was regenerated. Not a pass. `v0.1.0` was not moved. |
| R5 | Cold and warm paired suite | NOT_RUN | On `sha256:7aaac3c4`, cold and warm cancel and cancel-service both cancelled the scale-set sleep step, and both cache runs restored. Class wave compose `37196069739` through secret `37196131679` is in flight. Not a pass. |
| R6 | Docker capability suite and published macOS install | NOT_RUN | launchd program is a local binary, not a release asset |
| R7 | Required checks and protected main | NOT_RUN | PR 2085 not merged |
| R8 | P2 docs, diagnostics, PR dispositions | NOT_RUN | dispositions recorded under R0; remaining P2 work open |
