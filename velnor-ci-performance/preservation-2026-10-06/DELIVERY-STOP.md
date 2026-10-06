# Performance goal stop delivery — 2026-10-06

The performance goal is paused. This record preserves recoverable work and evidence only; it does not claim completion, product qualification, release acceptance, deployment, or a measured performance improvement. Implementation, research, tests, CI polling, and merges stopped on the supervisor's instruction. A process check found no active Cargo, rustc, Nextest, Clippy, or Alint process.

## Preserved remote refs

The following refs were pushed and their exact remote object IDs were verified. WIP refs preserve source bytes; they do not imply acceptance.

| Ref | Commit | Tree | Parent(s) | Scope/status |
|---|---|---|---|---|
| `codex/orchestrator-phase-timing` | `04399c26ab78bc0b353ef332b8af6ef1fe001f22` | `8115add766dcfd365a5d8a4ad310bb2696b10de4` | `425399520a47a42b14a3fd7aff298f97badb79b3` | 13-path telemetry WIP, including narrow Nextest output configuration; tests below passed, Clippy incomplete/failed. |
| `codex/image-release-exact-source-wip-20261006` | `3ccba5893c1253777842a910c15cf0fa5f69deb5` | `de2152fb1d1caafa83db04c88d7eeea61268f177` | `425399520a47a42b14a3fd7aff298f97badb79b3` | 2-path incomplete image/source gate WIP; untested, no publication acceptance. |
| `codex/local-release-manifest-wip-20261006` | `6ab40c27da5a41a040b97edbedc5ae323279ca68` | `9246db893611b2288df9b9d95f103c51bfc169c5` | `1caa21848df513f21718bde6f92c828dd4deb666` | 58-path parked verifier/CLI WIP; stale and unverified. |
| `codex/product-release-current-main-wip-20261006` | `76c9840edb2cfa33629f6b1047079096bb8e87af` | `2fd3306df6ed976151220bd76dc20364eaaec9b5` | `bfadc1f37d540344ac723651c4195dd68a8188e5` | 6-path product WIP; no native ABI or publication acceptance. |
| `codex/source-bound-conflict-preservation-20261006` | `a6abb98d0b00f3a659de28339520e4c55d6c69af` | `88ec90bb0408e93ab645bb01808830503f832bec` | `9e86fb6ad56d19ce146d7588ec82c2cda84b8806` | Conflict recovery archive; original conflict was not resolved. |
| `codex/agent-model-configuration` | `0e86c05611a594cd3b04e3a45225afb2bfae7094` | `6d64f17c19f5a0a9aa4e3687a48b16886e2d2198` | Prior ref `075284a03ae1858d7fabd99380efde4b3c3af435` | Model-policy branch fast-forwarded and remotely verified. |
| `codex/archive-fixture-reproduction` | `9ece72f0860bfba8a835d3062f0f555aa5954933` | — | — | Previously verified preserved checkpoint. |
| `codex/pr95-dogfood-goldens` | `9942c7039bce0b3bb4ab9f4e159530a9175bd27d` | — | — | Previously verified preserved checkpoint. |
| `codex/g0-generator-publication` | `71494f442b4e8a2419a8e9b5e29ba86fbd77afad` | — | — | Previously verified preserved checkpoint. |

The conflicted original worktree `/Users/donbeave/Projects/tailrocks/velnor-release-automation` remains intact. Its archive is `/tmp/velnor-source-bound-conflict-snapshot-20261006.tar.gz`, SHA-256 `bf9a003ed279ad37e269d1fac2851c967de25266afecdb951cc432dc842fa8b2`; the remote recovery commit records the manifest, worktree/index statuses, staged and unstaged diffs, conflict records, visible worktree bytes, and index-stage blobs. No conflict resolution was attempted.

## Evidence bytes and outcomes

The `velnor-ci-performance/` tree from the primary worktree is copied into this branch, including the existing goal/specification, evidence indexes, timing tables, probe notes, and Docker capture archive. The following selected log bytes are also included under `logs/`; SHA-256 values are listed to bind the copies:

| File | SHA-256 | Result/limit |
|---|---|---|
| `cold-parity-r2.log` | `0a419f3e8a35bac8b448066429e696299d99a8e24ce350b16fc5d1ffb4e2cad0` | Focused cold/parity 2/2; 1204 skipped; UUID `025b4fc2-9c31-47aa-b11c-9b7cd77e0c96`. No performance improvement inferred. |
| `request-tofu-r1.log` | `c41ed63ee73a7c1dd911af8f77cffe252f5c2f8cb4fac61923a463d45bd7d91e` | Selected request/ToFu tests 36/36; 1170 skipped; UUID `d5f39e82-9b03-4741-adbc-bd6cfc9a335a`. |
| `clippy-r1-stopped.log` | `bb43157055b382fc74083661fffdc0daec4385b6266155a0c67bccbab1730c6f` | Stopped with six warnings-as-errors (five `needless_option_as_deref`, one `doc_markdown`); no Clippy pass. |
| `cold-r1.log`, `compile-failure-r1.log` | `4208a18b7392dcb4fd47f447b57a658ee49812d1dbffea758837205a4409d107` | Same initial compile-failure bytes, preserved twice under their existing labels. |
| `pr95-cli-failed-job-111923226078.log` | `51b24d5508e6ba439091a952489aaf77a9f934777612514d2e29dcfe9f1c05f8` | Earlier remote CLI job failure; not attributed to uncommitted image WIP. |
| `pr95-cli-targeted.log` | `f3cd904aac6209d7d85ba3900832c86da3006015794021f82930a1bfcec3bc29` | Targeted CLI test 1/1; UUID `b8326858-1973-411c-bfad-602a8ea80341`. |
| `pr95-goldens-five-case.log` | `f9f7d45211fe7c6caa15ae755fa743666289054d857f8937a2c263a8fa537d54` | Five-case golden comparison passed. |

The telemetry cold run emitted `prepare_us=134597`, `metadata_commands=1`, `metadata_run_us=66957`, `metadata_parse_us=284`, `generator_sha_calls=1`, and `generator_sha_us=332351`; this is one local observation, not an improvement claim. A full run stopped after a macOS `root_escape` fixture failure. The log is retained in the existing performance evidence tree if present; its partial counts must not be described as a full-suite pass.

The selected logs above and the complete copied performance-evidence directory are remotely preserved. This does **not** claim every temporary runtime/session log, API response, or worker message was archived as bytes. Session logs and goal packet files were intentionally left in place. Worker G7 research is summarized here: current producer source at `aae1ff63b492d33b7faee218c51799257664b4b1` already performs three-target actual-byte/sidecar hashing, source-bound provenance and manifest checks, exact artifact-ID handling, smoke, and immutable publication flow. Open limitations include native Linux glibc-floor and macOS SDK/linker qualification, unresolved five-asset naming/policy, no downloaded/executed product binaries, and no current live host identity/access tuple. The repository/protocol evidence did not identify a host deployment endpoint. Consumer run `37311577004` retained a Tron/AMQ failure with cause unavailable and an Ethereum cancellation after a successful shared step/artifact; neither is accepted performance/cache-writer proof.

The preservation archive intentionally keeps source logs and CSV evidence byte-for-byte; `git diff --check` reports existing trailing whitespace in those archived data files. It is not a source-format validation result.

## Local state retained and cleanup

This branch provides the delivery archive; its exact commit/ref is reported with the final checkpoint. `.codex/state_5.sqlite` is a zero-byte local application-state file and is intentionally excluded. Original untracked `scripts/local-cargo.sh`, `scripts/test-local-cargo.sh`, and `velnor-ci-performance/` remain in the primary worktree.

After this delivery ref is pushed and verified, remove only these seven goal-owned temporary worktrees whose content is already preserved remotely: `/private/tmp/velnor-orchestrator-phase-timing`, `/private/tmp/velnor-image-release-source`, `/private/tmp/velnor-local-release-manifest-verify`, `/private/tmp/velnor-pr95-dogfood-goldens`, `/private/tmp/velnor-archive-fixture-reproduction`, `/private/tmp/velnor-source-bound-conflict-preservation-20261006`, and this delivery worktree. Do not remove the primary worktree, consumer worktree, long-lived product/release worktrees, session logs, or goal packets.

`git stash list` was empty. Detached experiment worktrees and local-only branches were not exhaustively audited for remote reachability and remain untouched; no claim is made that they were preserved remotely. Known local branch heads lacking a directly verified preservation ref at stop included `codex/nextest-bench-output` (`c6b1...`), `codex/g0-release-manifest-t1` (`071f...`), `codex/v0111-draft-recovery-aea2` (`aea2...`), and `codex/generator-manifest-contract` (`7e164...`). The long-lived conflict worktree also remains because preserving its source is safer than deleting it. The primary tree retains the untracked state/scripts/evidence noted above.
