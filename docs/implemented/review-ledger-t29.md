# T29 review ledger: independent-review disposition

Audited head: `e32df58cf2b6631aaa36ed6ccc5a67f70a2c49e1`
(`origin/feat/native-opentofu` at review time; all three finding
reports agree on this head). Disposition branch:
`origin/feat/native-opentofu`, commits c1–c9 below, all DCO-signed,
plain-pushed, per-commit gates green (c9 is a post-ledger pin test
for the c7 A8 fix; no verdict changes).

Sources: `threads/review-t29-arch/report.md` (A1–A10: 1 blocker /
2 major / 4 minor / 3 notes), `threads/review-t29-sec/report.md`
(B1–B17: 0 / 3 / 5 / 9), `threads/review-t29-perf/report.md`
(C1–C4: 0 / 0 / 0 / 4). 31 findings, one overlap (A1 == B1, same
root cause) → 30 distinct dispositions; the ledger carries all 31
IDs with the shared fixing SHA cross-referenced.

Verdict taxonomy: **Fixed** = behavior/docs/test change landed with
the cited SHA. **Accepted** = no behavior change; evidence and
rationale recorded here (the brief's rejected-with-evidence class).

## Fixing commits

| Commit | SHA | Scope |
| --- | --- | --- |
| c1 | `1c2f3b29c907d3621932363d6fe5ea3d4c936dee` | A1/B1/A6 payload + pin flip + realbin argv + lock gate |
| c2 | `e8de18288af4e274c614f7d3a487fe45c23c2c1a` | A2 reserved-root rejection |
| c3 | `c1e8c1449bc38bee8774c912db77fad6e550234e` | A3 eight-crate docs + pointers |
| c4 | `6cc2c3175c161f13e58e4867d567372c827b6df9` | B2 hashes parsing + tamper pins |
| c5 | `a094f9c0793ac36e2fcb2389c91711ae37eac2f7` | B3 model-only downgrade |
| c6 | `efe73f69aaab81819c5c321487566695e629ce29` | Minors A4/A5/A7/B4/B5/B6/B7 |
| c7 | `63b209c37b7964e604de2c7cac43b865700ee5be` | Notes batch + T21 rename |
| c8 | `af0d84a9b22c0a543df410800f1003ac3d853795` | This ledger |
| c9 | this commit | A8 pin test (post-ledger; verdicts unchanged) |

## Verdicts

| ID | Requirement | Verdict | Commit |
| --- | --- | --- | --- |
| A1 | init payload missing `-lockfile=readonly` (blocker) | Fixed: flag added, exact-argv pin, exclusion pin flipped, realbin runs product argv, planning fails on missing provider lock | c1 |
| B1 | readonly gap = trust boundary (same root cause as A1) | Fixed: shared fix with A1 (same SHA) | c1 |
| A2 | `root` spelling collides with repo-root key | Fixed: `reserved_root_key` rejection + negative pin | c2 |
| A3 | seven-crate MUST false + 3 stale pointers | Fixed: eight crates (row + edge), 3 pointers amended | c3 |
| B2 | lockfile hashes never parsed | Fixed: non-empty `hashes` required per provider block + 4-case tamper battery | c4 |
| B3 | restore verification unwired on live path | Fixed via allowed downgrade: model-only docs/names; residual recorded (§4) | c5 |
| A4 | F4 prose stale, no enforcement | Fixed: decision-based F4 + renderer symbol-leak test | c6 |
| A5 | staged lane jobs skip on predecessor failure | Fixed: `if: always()` on predecessor-gaining jobs + YAML pin | c6 |
| A6 | init/validate payloads missing `-no-color` | Fixed: with A1/B1 (same payload edit + pin) | c1 |
| A7 | fmt classification in two unpinned lists | Fixed: family-derivation consistency pin (23-name corpus) | c6 |
| B4 | reads + lock capture follow symlinks | Fixed: symlink refusal + unreadable capture + unix negatives | c6 |
| B5 | argv/env rejection paths unpinned | Fixed: env pins + stable token pins (argv behavior was already pinned; tokens were not) | c6 |
| B6 | cache-key gate rejections unpinned | Fixed: unsafe/leading-dash/overlong pins + depth-budget note | c6 |
| B7 | provider key omits trust segment (§4.4 drift) | Fixed: §4.4 amended to the static-key reality with rationale | c6 |
| B8 | remote module sources recorded never pinned | Accepted: V1 limitation recorded; warn deferred (§4) | — |
| A8 | `allow_task_reuse` true while gate disables | Fixed: init/validate proposals say false + gate comment | c7 |
| A9 | `== Some(Stack::Tofu)` vs exhaustive matches | Accepted: record (§3) | — |
| A10 | `strategy.max-parallel` declarative-only | Accepted: T32 E2E observation carried (§4) | — |
| B9 | payload argv accepts traversal (unreachable) | Fixed: segment-exact `..` rejection + pin | c7 |
| B10 | root grammar permits `$()`/quotes/`;` (contained) | Accepted: record, no action (§3) | — |
| B11 | secrets-substring rationale prose-only | Fixed: non-flagging boundary pinned in test | c7 |
| B12 | never-archive under-inclusion (contained) | Accepted: scope documented in code + here (§3) | — |
| B13 | no fork-PR isolation composition test | Fixed: fork trust + restore-only roundtrip pin | c7 |
| B14 | toolchain digest evidence-only | Accepted: never claim byte-verified (§3) | — |
| B15 | needs-set trust root process-only | Accepted with note (§3) | — |
| B16 | qual-record FIXME self-trip | Fixed: reworded off the sweep | c7 |
| B17 | H5 allowlist unimplemented, fail-closed | Accepted: no functional impact confirmed (§3) | — |
| C1 | `plan_ms` fixture-commit impurity | Fixed: plan+fixture-commit method note | c7 |
| C2 | `BENCH_ROOTS` wrong-file cite | Fixed: cite corrected to `impl_tofu_t24_bench.rs` | c7 |
| C3 | plan scaling stops at 40 roots | Fixed: 100-root matrix-budget fail-closed pin | c7 |
| C4 | FileCache memcpy tradeoff | Accepted: observation recorded, not fixed (§3) | — |

Counts: 31 verdicts — 22 fixed, 9 accepted-with-evidence
(B8, A9, A10, B10, B12, B14, B15, B17, C4), 0 still-open.

## §1 Blocker + majors

**A1/B1 (c1).** `tofu_payload_argv` for `InitForValidate` is now
`init -backend=false -input=false -lockfile=readonly -no-color`
(byte-exact pin `payloads_pin_readonly_and_no_color`:
readonly exactly once on init, no lockfile token elsewhere,
`-no-color` last on all kinds; the S7 exclusion pin is deleted).
Discovery fails planning when a provider root lacks a committed
lock (`require_committed_provider_lock`: `missing_committed_lock`
/ `unreadable_committed_lock` with the manual `tofu providers
lock` remediation; provider-free roots pass, malformed configs
abstain). T27 realbin runs execute product argv (new mise→tofu
test-only dev-dep): dead-mirror stages a complete lock, the live
test re-downloads through readonly init and proves the lock
byte-identical. T14/T22/T27-render pins rewritten to the mandated
fail-closed contract. Contract §1 outcome #2 and §4.4's "restore
still faces readonly init" now hold; no S7 behavior-change commit
remains outstanding.

**A2 (c2).** `check_root` rejects the exact `root` spelling with
`reserved_root_key` (mirrors rust `manifest_key_for_cargo_manifest`);
`qualify_roots` inherits it via validation. Pin covers the lone
spelling, the `.`+`root` pair, and the `roots`/`a/root` near-misses
that stay valid. Fail-before proven by stash.

**A3 (c3, docs-only).** `architecture.md`: eight-crate MUST,
orchestrator→tofu edge, tofu ownership row. Pointers:
`requirements-evidence.md` (eight + live test name),
`deviations.md` (8-crate), `docs/content/docs/implemented/gate-0-repository-contract.mdx`
(annotated as the `bdfffb9` historical record with the live test
name; history not rewritten).

**B2 (c4).** The native walk counts `hashes` entries per top-level
`provider` block (count-only, no budget impact; extraction lives
in `lockfile.rs`, `parser.rs` at 390 lines). `inspect_lockfile`
emits `tofu_lockfile_unpinned_hashes` (manual remediation) naming
exactly the unpinned providers; corrupt keeps its early return,
unpinned and stale accumulate. Pins: silence on complete pins,
4-case tamper battery (stripped/emptied/non-array/added-unpinned),
malformed-config independence. Garbage-but-nonempty shapes stay
tofu's runtime concern (readonly init fails closed on hash
mismatch); recorded, not widened.

**B3 (c5).** Downgrade path (explicitly allowed): zero production
consumers of `verify_provider_restore`/`classify_restore` exist
(task reports carry no restore observations), so wiring a live
post-restore step needs report-schema work beyond disposition
scope. Docs and the T22/T27 test names now say model-only and
name the live chain (exact-key restore → lock-verified readonly
init → mandatory validate). Residual in §4.

## §2 Minors

**A4 (c6).** F4 amended to the decision-based criterion
(renderer/transport/CLI own no tofu domain decisions);
`impl_renderer_tofu_leak` pins 14 forbidden symbols absent from
the three renderer tofu sources (`include_str!`, precedented).
CLI src verified zero tofu; `tofu_exec` verified a pure ctor.

**A5 (c6).** Jobs that gain a lane predecessor carry
`if: always()` (`STAGED_TOFU_JOB_CONDITION`); first-in-lane jobs
keep the plan gate. Pinned at IR level and in generated YAML
(4-space job-level match; the 8-space step-level upload `if:`
exists in every job). Scope note: predecessor-gaining jobs only,
not every staged job — the condition exists to survive lane
failure, and the plan gate stays intact for the rest.

**A7 (c6).** `fmt_inclusion_matches_family_derivation` pins the
S2 set against a family+dialect oracle over 23 names (native
configs/overrides, tests, non-JSON vars; JSON/lock/other/excluded
never).

**B4 (c6).** `read_raw` refuses symlinks without reading
(`symlink_refused`, cached like any failure); lock capture treats
symlinked locks as unreadable (verify fails closed). Unix
negative tests for both; fail-before proven by stash.

**B5 (c6).** Finding partially overstated: argv behaviors were
already pinned (`argv_validation_rejects_policy_violations`) —
only the tokens were not. Added: `validate_env` pins (bad keys,
bad values, clean/empty pass) and stable-token pins for all
seven argv paths plus both env tokens.

**B6 (c6).** Rejection pins for unsafe roots, leading-dash roots
(reachable: `validate_fetch_root` allows them), and overlong keys
from deep nesting (plus an ordinary-nesting pass); depth-budget
note on the key constructor (the 512 B cap is the budget; no
separate depth cap). The f2a fetch-verb sweep scrubs the asserted
`unsafe_fetch_root` error-code literal.

**B7 (c6).** Amended §4.4 (either option closed the drift):
static workflow keys carry no trust segment because event trust
is unknowable at generation — the established rust sources-key
precedent, and `cache_key()` (trust-segmented) has no live-path
callers. Isolation rationale recorded (exact-key restore,
push-gated saves, branch scoping, lock-verified init, mandatory
validate).

**B8 (accepted).** Remote module sources stay recorded-but-
unpinned for V1 (sources flow into digests/closure; no pin
semantics exist). The optional unpinned-ref warning is deferred
(see §4); no code change.

## §3 Accepted notes (evidence)

**A9.** `== Some(Stack::Tofu)` guards accepted: closed-`Stack`
dispatch arms own exhaustiveness at the waist; boolean guards at
call sites are the established local style. No change.

**A10.** `strategy.max-parallel` declarative-only accepted; no
product enforcement exists at this layer. Carried as a T32 E2E
observation (see §4).

**B10.** Root grammar permits `$()`/quotes/`;`, contained:
downstream argv/env/YAML layers reject or quote them
(B5/B9 pins, renderer expression rules). No grammar change.

**B12.** Never-archive under-inclusion accepted and scoped: name
markers only (`credentials`, `.tfstate`, `.tfplan`); secret/
token/secrets substrings stay archivable (B11 pin). Contained by
the fixed archive subset. Scope sentence added to the marker
docs (c7); no behavior change.

**B14.** Toolchain digests stay evidence-only: verified no
byte-verified claim exists for them (sole repo `byte-verified`
hit is reuse-stage test prose about output observations, a
different surface). Never claim otherwise. No change.

**B15.** Needs-set trust root accepted as process-only (branch
protection + required checks carry cross-run trust; recorded in
`needs_channel.rs`). No change.

**B17.** H5 provider-symlink allowlist unimplemented, fail-closed:
no allowlist impl exists on any tofu path (remaining `allowlist`
hits are save-policy/source-subset), closure rejects any symlink,
and B4 refuses symlink reads — so nothing functionally depends on
the allowlist. Confirmed no impact; no change.

**C4.** FileCache memcpy tradeoff recorded as an observation
(small-file re-reads favor simplicity over zero-copy; no
measurement shows it load-bearing). Not fixed by instruction.

## §4 Residuals and carried items (owners)

- **B3 residual:** no live-path post-restore verification step;
  the live chain is exact-key restore → lock-verified readonly
  init → mandatory validate. A reports-carrying-observations lane
  could wire the model gate later. Owner: coordinator / future
  lane. Not open: the downgrade disposition is complete.
- **B8 deferred:** optional warn on unpinned remote refs. Owner:
  coordinator. Not open: V1 accepts recorded-but-unpinned.
- **A10 carried:** observe `max-parallel` behavior in the T32 E2E
  preview. Owner: T32 lane.

Still-open findings: 0.

## §5 Gate evidence (per commit)

Every commit: `cargo fmt --check` empty, workspace clippy `-D
warnings` 0, full `nextest --locked --workspace`, goldens check,
`cargo deny check`, `check-freshness.sh`, `alint
check --fail-on-warning` (3 pre-existing infos steady).

| Commit | nextest run / passed / skipped | Notes |
| --- | --- | --- |
| c1 | 2617 / 2617 / 1 | +1 net (lock-plans pin); dogfood `plan.txt` re-blessed 1 line (new mise test-only edge; generated tree identical); live-registry test re-run green |
| c2 | 2618 / 2618 / 1 | +1 (reserved-root pin) |
| c3 | 2618 / 2618 / 1 | +0 (docs-only) |
| c4 | 2620 / 2619 / 1 | +2 (tamper pins); 1 failure = known environmental p12_live 403 rate-limit on the Swatinem/rust-cache probe (fails identically at base) |
| c5 | 2620 / 2619 / 1 | +0 (renames); same single environmental failure |
| c6 | 2630 / 2630 / 1 | +10 (A4/A5/A7/B4×2/B5×2/B6×3); p12_live clean |
| c7 | 2633 / 2633 / 1 | +3 (B9/B13/C3); includes T21 model-checks rename |
| c8 | 2633 / 2633 / 1 | docs-only (ledger) |
| c9 | 2634 / 2634 / 1 | +1 (A8 pin); verdicts unchanged |

Fail-before observed (stash) for: A2, A5, B2 (new-symbol
pins), B4 (both), B9. New-symbol pins (B2 const)
additionally fail to compile at base by construction.
Composition pins over unchanged behavior (B6, B13, C3) are green at
base by design; their value is regression coverage.
