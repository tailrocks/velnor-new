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
| G1-migrate | Schema migrate round-trip, idempotence, unknown fields | NOT_RUN | pending contract tests |
| G1-labels | Illegal hosted label on scale-set selector fails | NOT_RUN | pending contract tests |
| G1-capacity | Occupancy never exceeds N; no release without cleanup proof | NOT_RUN | pending core tests |
| G1-wire | Null/omit, message 0, empty poll, partial acquire, single-flight refresh | NOT_RUN | pending protocol fixtures |
| G2-journal | Crash before and after each external step; reopen real database | NOT_RUN | pending host tests |
| G2-uncertain | Uncertain acquire and delete keep capacity | NOT_RUN | pending host tests |
| G3-isolation | Private DinD, no outer socket/home, canary absent from metadata | NOT_RUN | pending spec tests; live inspect not run |
| G3-ownership | Foreign objects survive; id mismatch quarantines | NOT_RUN | pending host tests |
| G4-live | Real GitHub job on official runner and scale-set labels | NOT_RUN | no live session attempted yet |
| G5-routing | Schema 1 unchanged; `both` duplicates verification only | NOT_RUN | pending generator tests |
| G5-compare | Duplicate, missing lane, swapped artifact, unsafe archive fail closed | NOT_RUN | pending compare tests |
| G6-launchd | User LaunchAgent foreground `daemon run`; second daemon fails | NOT_RUN | launchd not driven yet |
| G6-binary | `velnor-host` help and not-ready status | NOT_RUN | binary not built yet |
| G7-publish | Published generator, image, and macOS binary consumed by ChainArgos | NOT_RUN | no release attempted |
| G7-paired | Hosted baseline, N=1 canary, N=2, cold and warm paired runs | NOT_RUN | no consumer workflow run |
| G8-merge | Final main uses pinned published generator; required checks kept | NOT_RUN | no promotion |
