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
| G7-publish | Published generator, image, and macOS binary consumed by ChainArgos | NOT_RUN | image `37102027384` and macOS `37102029367` published GitHub release assets at `19a43f5` (no GHCR push). PR 16 published tag `generator-d40868152f7fe0106e3ede858a411f502f00810f`; git tag `v0.1.0` remains `c57c700459bbe1549fe7eedcb7d8689585c38986`. ChainArgos pins generator `d40868152f7fe0106e3ede858a411f502f00810f`. Installed LaunchAgent is local `030363df06a99355afc9534fa42868faf7f39500`, not a published macos-binary-release asset |
| G7-paired | Hosted baseline, N=1 canary, N=2, cold and warm paired runs | NOT_RUN | hosted baseline `37113202410` succeeded; main push `37114238559` failed only on Post Restore MBX objects; N=1 run `37117721384` succeeded and is not this row; N=2, cold, and warm are absent |
| G8-merge | Final main uses pinned published generator; required checks kept | NOT_RUN | no promotion |
