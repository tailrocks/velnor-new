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
| G3-isolation | Private DinD, no outer socket/home, canary absent from metadata | NOT_RUN | `g3-matrix.txt`: private mounts and canary count 0; live exec missed because dummy JIT exited |
| G3-ownership | Foreign objects survive; id mismatch quarantines | NOT_RUN | foreign id stayed; `delete_decision` is not wired to a public Docker delete |
| G4-live | Real GitHub job on official runner and scale-set labels | PASS | job `111084145716` runner `m100000009`; one-class jobs in `evidence.md`, including cache rerun `37093907324` and same-port run `37096417428`; full G4 suite still open |
| G5-routing | Schema 1 unchanged; `both` duplicates verification only | PASS | `impl_schema2_routing.rs` at `40e08e8`; goldens unchanged |
| G5-compare | Duplicate, missing lane, swapped artifact, unsafe archive fail closed | PASS | `compare_tests.rs` at `658154c` |
| G6-launchd | User LaunchAgent foreground `daemon run`; second daemon fails | PASS | `launchctl print gui/501` at 2026-10-03: absolute `daemon run`, `forks = 0`; second `daemon run` exit 1; bootout removed the job |
| G6-binary | `velnor-host` help and not-ready status | PASS | `help_exits_success_and_a_bad_command_does_not`; status JSON `waiting_for_credentials` |
| G7-publish | Published generator, image, and macOS binary consumed by ChainArgos | NOT_RUN | `image-release.yml` and `macos-binary-release.yml` `workflow_dispatch` each HTTP 404, not on the default branch; not retried; ChainArgos not updated |
| G7-paired | Hosted baseline, N=1 canary, N=2, cold and warm paired runs | NOT_RUN | no consumer workflow run |
| G8-merge | Final main uses pinned published generator; required checks kept | NOT_RUN | no promotion |
