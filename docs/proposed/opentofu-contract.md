# Velnor Actions: native OpenTofu contract (adopted)

**Status:** Proposed implementation contract. Unchecked ledger items are not
implemented or verified.
**Parent:** `velnor-actions-opentofu-spec.md` (research 2026-10-01) plus the
normative tightenings in §§4–6 below. This file is the single adopted copy;
do not maintain a diverging duplicate (goal §"Specification").
**Ledger:** [`../reviews/opentofu-evidence.md`](../reviews/opentofu-evidence.md)
(T01–T36 evidence rows; the SOLE checklist tracker — this contract does not
duplicate it).
**Phase A branch:** `feat/native-opentofu` from `origin/main` 106bfd7.

## 1. Outcome and non-negotiable scope

A real OpenTofu repository gets generated GitHub Actions with no Cargo
project, no custom-shell escape hatch, and no lost IaC verification:

1. `tofu fmt -check` over the established formatting scope.
2. Per-root `init -backend=false -input=false -lockfile=readonly`.
3. Real `tofu validate` in the initialized root.
4. Failure/cancellation/missing-report/preparation-error propagation to
   Required.
5. Verified workflow on the consumer PR **and** the merged main commit.

A green Plan/Actionlint/Required skeleton with no tofu obligations is a
failed migration. Initially `fmt` + validation-only `init` + `validate`
only; no `plan`/`apply`/`destroy`/`import`/state/`tofu test`.

## 2. Re-grounded identities (spec anchors → live values)

The spec's anchors (`47a60bff` → `9c3a1983`, consumer `04e6e9d`) are
historical. Live values (re-fetched 2026-10-02):

| Anchor | Spec value | Live value | Evidence |
|---|---|---|---|
| Producer PR #1 | open branch `docs/velnor-actions-spec` | **MERGED** 2026-10-01T17:39:05Z, squash `8b268d17b5c54eb0120763fca2b010b8b695e91c` (parent = base `58323453ce6a7eb4841d56f2bf16b897eabc6ced`); branch deleted | ws1 §2; `gh pr view 1` |
| PRs #2/#3/#4 | — | merged into the PR branch (filename `release.yml`, Required gate, publish env), carried to main via the #1 squash | ci-release §"Publishing" |
| `origin/main` | — | `106bfd7bdc721d29671132eaabf81c86154e1835`: #8 BR-26 neutral boundary, #7 BR-28 tofu fixtures, #5 fixes, #1; CI green (push run 36916298152) | ws1 §3; `gh run list` |
| Consumer main | `04e6e9d` | `f0eedf3c14ca78ab2967636d1d4dde613f16578d` (#36); **RED**: Required merge-v1 exits 1 on empty plan (`no_work` fail-closed, run 36889650178). Root cause proven + docker-repro byte-identical: seed6 `plan-v1` emits zero obligations (registry knows only `Stack::Rust`) → `decide()` = `NoWork` → `merge_passed()` false → silent exit 1 in `dispatch.rs`; verdict logic byte-identical seed6→main, so a seed bump alone keeps main RED — fix-forward is the tofu extension (never flip `no_work` to pass) | ws1 §5/§7; mergev1-diag |
| Consumer PR dispositions | — | #34 close superseded (no merge): main #36 carries the whole migration, `ci.yml` + `config.toml` blob SHAs identical to PR head `30a9128`; #31 close stale: termpane already `["DCO","Required"]` on main, merging would wedge it on never-reported checks | mergev1-diag |
| Consumer PRs | — | #34 OPEN superseded by #36 (unique delta: 1-line `default_branch` pin); #31 stale 2026-09-21 | ci-release |
| Pinned artifact | seed6 @`517c1d6` | consumer still on seed6 (`b074dcb2…fb663`, 6941680 B); seeds 7/8/9 supersede; seed9 @`09b443e` (`4841b45f…`, 7444008 B, gains required `commit`) still stale vs main | ws1 §7; ci-release |
| `release.yml` | rename delta | **On main**: `RELEASE_WORKFLOW_PATH`, emitted only under `consumer-v1` + `[stacks.rust.release].enabled`; local `fix/release-workflow-filename` is a stale duplicate | ws1 §4 |
| Tofu pin | 1.13.0 / consumer 1.12.5 | qualify **`1.13.1`** exact (§5) | tofu-release; ws4 §7 |
| Parser | unqualified | `hcl-rs` 0.19.8 + `serde_json`; `hcl-edit` 0.9.7 deferred; MSRV unstated → compile-gate at 1.98.1 in T10 | ws3 §8 |

Local main at Phase-A start: `58323453ce6a7eb4841d56f2bf16b897eabc6ced`
(clean, 4 behind); `feat/native-opentofu` starts at `106bfd7`.
Linked worktree `velnor-new-release` + local-only branches are preserved,
never reset/deleted.

## 3. Architecture (spec §4, re-grounded at 106bfd7)

`origin/main` already lands the discovery/proposal seam (BR-26): neutral
`FileIndex`, `DetectorEntry` fn-pointer registry, `StackCandidate`,
`DetectedProject`, `ProposedTask` → `TaskNode`/`TaskGraph`, closed `Stack`
dispatch. BR-28 lands the tofu fixture corpus (10 dirs, 77 files, no code).

Add **`crates/velnor-actions-tofu`** (Phase B): stack ID `tofu`, Mise tool
`opentofu`, executable `tofu` — distinct concepts, no aliases. Dependency
direction: orchestrator → {contract, rust, tofu, mise, actionlint,
renderer}; all adapters → contract only. Tofu owns domain semantics; Mise
owns execution; renderer owns representation; orchestrator composes.

Rust constraints (spec §3 + hard invariants 10–12) hold throughout,
detailed in `rust-quality-contract.md`: refactor-first with golden parity,
KISS/YAGNI, `unsafe` forbidden, no product `unwrap`/`expect`/`panic!`,
committed `Cargo.lock`, exact pins, 400/150/80 line gates,
`deny_unknown_fields` strictness.

### 3.1 Seam resolution: fn-pointers + dispatch arms, no trait

**Verdict (Phase A, evidence at live head): extend the `DETECTORS`
fn-pointer style with closed-`Stack` dispatch arms. Do NOT introduce a
`StackAdapter` trait.**

Evidence:

1. The live head already converged on this seam: `DetectorEntry =
   (&'static str, u32, fn(&FileIndex) -> Vec<StackCandidate>)`
   (`contract/src/discover/registry.rs:14`), `DETECTORS` array
   (`orchestrator/src/discover.rs:127`), and exhaustive
   `match Stack::require_known` arms in `closure.rs:30`,
   `toolchain_inputs_for` (`identities.rs:304`), `manifest_for_candidate`
   (`inventory.rs:48`), `extension_schema_for_stack`.
2. Code docs prescribe dispatch arms for the next stack
   (`toolchain_inputs_for`: "A future stack gets its own
   toolchain/format-identity path and dispatch arm").
3. Spec §3.2 permits a trait only for a demonstrated interchangeable
   capability; Phase A has one stack, so a 19-method single-implementor
   trait is speculative generality (YAGNI).
4. Exhaustive matches give the same compiler-guided migration checklist
   a trait would.
5. The rust crate cannot host derive/shard logic (it would need a Mise
   dependency, violating the arch diagram); the orchestrator keeps one
   rust-adaptation module (`derive_groups.rs`, the sole non-test
   `TaskGroup` user — F1).

### 3.2 Falsification gates (WS2b F1–F7, verdicts at live head)

| Gate | Verdict | Evidence |
|---|---|---|
| F1 zero non-test `TaskGroup` in orchestrator outside one rust-adaptation module | **PASS** | only `derive_groups.rs` (feature/shard derivation); `select_affected` + `cover_identity_fixtures` uses are `#[cfg(test)]` |
| F2 zero new `serde_json::Value` on discovery/proposal/identity paths; tofu extension gets a required-slot validator | **PASS** (constraint) | zero hits at live head; refactor adds none; `tofu-task-identity-v1` validator lands in Phase B |
| F3 one reverse-closure owner over neutral edges; zero tofu graph/scheduler types | **PASS** | `contract::reverse_closure<N: Ord + Clone>` is the single owner; `rust/src/graph.rs` holds no copy; `select_affected` converts via rust-owned `local_edge_pairs` |
| F4 renderer/transport/CLI own no tofu domain decisions (argv/flags, root selection, lock/version/scope); no `RenderDriver` variant | **PASS** | renderer holds tofu representation only (step templates, writer election, layer arms), pinned by the `impl_renderer_tofu_leak` symbol test; CLI src zero tofu hits; mise `tofu_exec` pure ctor, no selection rules |
| F5 one `REGISTERED_STACKS`; neutral detector records; neutral identity inputs; 7→8 arch tests amended with enforcement intact | **PASS except T08** | M1 deletes the rust mirror; detectors already return `StackCandidate`; lane/platform/toolchain take `&ProposedTask`; T08 (eighth crate) is Phase-B-gated |
| F6 skew items re-verified | **PASS** | custom-task allowlist IS in `RustStackConfig` at live head (`stacks.rs:57-60`, spec F06 correct); `DetectorEntry` is `(&str,u32,fn)`; `CrateJob` keeps `package_id` + `"Rust / "` gate while `CrateObligation` is neutral; placeholder digest fails closed (`validate_identities`); `release.yml` on main |
| F7 consumer parity (recursive fmt + readonly backend-less init + validate; module→root selection; hosted labels; no `velnor-workflow` residue) | **PENDING Phase B/E** | requires tofu behavior; rationale recorded, not waived |

Known deferred warts (rationale, not waiver): `manifest_key_for_cargo_manifest`
stays in contract until the sharding-config schema move (its contract-internal
consumer `resources.rs` needs the Cargo-suffix rejection; moving the schema is
a versioned config change, owned by Phase B); `Discovery.workspaces`
Cargo-typing and render parameterization generalize on the tofu second use
(invariant 11), not speculatively in Phase A.

## 4. Normative tightenings

### 4.1 Tofu domain deltas S1–S10 (WS3, all probed on 1.12.5)

- **S1 precedence (MUST):** `.tofu`⊃`.tf` incl. parse suppression;
  `.tofu.json`⊃`.tf.json`; NO native↔JSON suppression (both load →
  duplicate error). Same for overrides. Effective-set = per-directory
  grouping by (basename, dialect), shadowed files dropped.
- **S2 fmt set (MUST):** inclusion = `.tf`, `.tofu`, `.tfvars`,
  `.tftest.hcl`, `.tofutest.hcl` (NOT JSON, NOT `.tofuvars`);
  exclusion predicate = "starts with `.`/`~`, or starts+ends with `#`";
  `-check` exit observed = 3 and implies `-write=false`. Never bare
  `fmt -list`/`fmt` for detection.
- **S3 argv (MUST):** payloads verified on 1.12.5; `-chdir` is global and
  precedes the subcommand. ADD `path.cwd` caveat finding for subdir roots
  (identity for `.`).
- **S4 lock diagnostics (MUST):** map readonly-init stderr `Error: Provider
  dependency changes detected` (exit 1) to the remediation message.
- **S5 init→validate edge (MUST):** grounded — validate without init fails
  `Missing required provider`.
- **S6 module grammar (MUST):** local ⇔ literal source starting `./`/`../`;
  absolute path = external package copy (conservative finding, not an edge);
  include JSON `module` blocks and override-file `module` blocks; dynamic
  source → conservative finding.
- **S7 legacy delta:** consumer `verify-policy.sh` runs bare fmt + bare
  validate (no init); explicit init + readonly lock are behavior-change
  commits, never refactor.
- **S8 parser (MUST):** `hcl-rs` 0.19.8 + `serde_json`; `hcl-edit` deferred
  behind a tofu-owned facade; byte/file/depth caps; malformed → typed error,
  never empty inventory; no expression evaluation.
- **S9 validate `-json`:** exit code authoritative; early non-JSON stdout
  possible → parse failure is a generic error, never success.
- **S10 lockfile:** root-level, providers-only; multi-platform hashes via
  `tofu providers lock` (maintenance path, outside readonly CI).

Config (`[stacks.tofu] roots`, schema 1, strict): REQUIRED non-empty sorted
duplicate-free `Vec<Utf8RepoRelDir>`; POSIX, no absolutes/control bytes;
`.` = repo root; reject `..`/`.`-segments/double slashes/symlink escapes;
lexical + canonical containment; root MUST contain ≥1 effective config file
post-precedence or error naming the root. No other keys in v1.
Detection evidence: STRONG (explicit table / `*.tofu*` / Mise opentofu /
required_version+legacy refs) vs WEAK (`.tf` only → require explicit table,
never silently claim) vs CONFLICT (terraform markers + STRONG → hard error).
Selection: base+head graph union; removed edges never drop callers; child
module dirs never auto-promoted to roots; cycles/missing-target = error;
unknown/dynamic/external → select ALL roots with recorded reason.

Task kinds `Fmt`/`InitForValidate`/`Validate`; IDs
`stack/tofu/root/<kind>/<config>`; argv with `-chdir` FIRST; Fmt independent,
Validate depends on same-root Init (shared private `TF_DATA_DIR`, one init
per root per attempt); one fmt invocation per non-overlapping scope.

### 4.2 Security MUSTs H1–H6 + M1–M6 (WS6)

- **H1 env allowlist (MUST):** default-deny per-purpose env for tofu
  children (ambient inheritance; §4.4 approves generated values). Denylist
  at minimum: `TF_LOG`/`TF_LOG_PATH`, `TF_PLUGIN_CACHE_DIR`,
  `TF_DATA_DIR`, `TF_WORKSPACE`, `TF_CLI_CONFIG_FILE`, `TF_REGISTRY_*`,
  `CHECKPOINT_*`, `GITHUB_*`/`GH_TOKEN`, `OP_*`/`OP_SERVICE_ACCOUNT_TOKEN`,
  cloud families (`AWS_*`, `GOOGLE_*`, `ARM_*`, `HCLOUD_*`, …),
  `HOME`/`XDG_*`, plus spec §7's `TF_CLI_ARGS*`/`TF_VAR_*`/`TF_TOKEN_*`/creds/
  lock-bypass. Negative tests with hostile ambient values.
- **H2 auto.tfvars (MUST):** enumerate committed `terraform.tfvars`/
  `*.auto.tfvars` per root into identity/selection; reject in PR validation
  roots with remediation, or treat validate output as attacker-influenced.
  Force-added fixture test.
- **H3 injection (MUST):** hash (hex), never interpolate, repo-derived cache-key
  components; job IDs `^[A-Za-z0-9_-]+$` via fixed mapping; reject `${{`,
  control bytes, newlines, oversized inputs at owner boundaries; renderer
  asserts no `${{` survives in emitted YAML.
- **H4 workflow commands (MUST):** escape (`%0A/%0D/%25/%3A/%2C`) or reject
  control sequences in tofu-derived annotations/summaries; validate
  `file`/`line` against the known index; never raw stderr into commands.
- **H5 symlinks (MUST):** canonicalize (realpath) every root/module/file at
  the filesystem boundary post-checkout and re-verify before exec; reject
  escapes; allowlist only job-private provider symlinks by cache-dir prefix.
  Escaping-symlink fixtures for root/edge/fmt-scope.
- **H6 fmt batches (MUST):** prove `--` support on the qualified binary; if
  absent, reject `-`-leading basenames; argv batches, never shell-joined;
  `-evil.tf` fixture.
- **M1:** tofu jobs get explicit least-privilege `permissions`, no
  `id-token:write`; never rely on permissive org defaults (tracked residual).
- **M2:** bound init wall-time + fetched bytes; readonly fails on new
  providers (tested); record module source/content identity (no reuse yet).
- **M3:** fail-closed merge for every report defect incl. missing plan +
  empty matrix on tofu-affecting PRs; keep `if-no-files-found:error`.
- **M4:** typed-constructor CLI config: `plugin_cache_dir` + `disable_checkpoint`
  only; no credentials/helpers/overrides/mirrors; controlled `HOME`.
- **M5:** tofu job steps contain no `secrets.*`/`github.token`, no `gh` tool
  (generator asserts; YAML grep test).
- **M6:** CI mise invocations use `--no-config --no-env --no-hooks` +
  explicit `opentofu@<exact>`; never `mise run` repo tasks; `mise.toml`-edit
  adversarial test. L1: never invoke `verify-policy.sh` from CI. L2:
  exact-key provider-cache restore (fork-PR isolation test).

### 4.3 OpenTofu 1.13.1 qualification (MUST)

Pin **`OPENTOFU_VERSION = "1.13.1"`** exact (no `v` prefix), source
`https://github.com/opentofu/opentofu/releases/tag/v1.13.1`
(published 2026-10-01T17:15:15Z; bugfix-only over 1.13.0).
Catalog digest = linux_amd64 **`.tar.gz`** sha256 (aqua backend, proven by
consumer `mise.lock`):
`378ada19d4bc70c43732004e8159be771b23b9a5afdf059e5f8a2b3fa2c70a69`.
Spec payload flags all verified at v1.13.1 (docs + `BindView`/`arguments`
source; note `fmt.mdx` omits `-no-color` — cite source). Qualify under
freshness class `tools` (age 1 day at check). Consumer compat: PASS
(`required_version >= 1.7.0` admits; no `base64gzip`/provisioner/WinRM
exposure; 32-bit N/A). Caveats: mise index lag (float never; pin exact);
**re-run `fmt -check` with the 1.13.1 binary** (canonical fmt may drift
across minors); macOS 13+ floor unconfirmed for dev machines.
`PinnedTool::Opentofu` (9th tool) + `opentofu` catalog field +
`EXPECTED_TOOLS` + inventory row (`source=…/releases/latest`) + `[tools]`
mirror; placeholder digest never proves the new tool (P03-8b).

### 4.4 Mise/cache/perf adoption (WS4 §§7–12)

Ownership: `rust-quality-contract.md` owns the Rust quality rules; §§4.4–4.5
state tofu adoption deltas by reference, never restating them.
Per-role tools: pure-tofu plan = opentofu + actionlint/shellcheck/zizmor
(no Rust/MBX/Nextest/components/Cargo fetch); `tofu-<root>` jobs = opentofu
only + provider-cache restore; actionlint/required/validators unchanged;
mixed = union; removal is a distinct behavior-change commit.
Provider cache: closed layer `tofu-providers`, key
`velnor-v1-tofu-providers-<target>-<tofu>-<root-slug>-${{hashFiles(...)}}`
(≤512 B), no trust-namespace segment in static keys: event trust is
unknowable at generation (like the rust sources key), so isolation
holds via exact-key restore, push-gated saves, and runner branch
scoping, and every tofu hit still faces lock-verified readonly init
plus mandatory validate; transport = plugin-cache dir
only (job-private `$RUNNER_TEMP/velnor/tofu-cache/<slug>`), restore before
init; per-root job saves only its own root-scoped key (plan never inits so
never saves); push-gated trusted save; restore still faces readonly init;
hit MUST still run init+validate (T23). Compiler reuse stays under
`rust-quality-contract.md` §3 (MBX profiles); this layer caches provider
artifacts only. Env: approved
`TF_IN_AUTOMATION=1`, `TF_INPUT=0`, per-root `TF_DATA_DIR`, generated
`TF_CLI_CONFIG_FILE`, `TF_PLUGIN_CACHE_DIR` + isolation quartet +
install-disable; deny `TF_CLI_ARGS*`, `TF_VAR_*`, `TF_TOKEN_*`/cloud creds,
`TF_PLUGIN_CACHE_MAY_BREAK_DEPENDENCY_LOCK_FILE`, `TF_WORKSPACE`, `TF_LOG*`,
`TF_CHECKPOINT*`/`CHECKPOINT_*`, ambient config/data/cache overrides,
`TF_REGISTRY_*`. H1's denylist governs ambient inheritance; the approved
names above are values the generator sets explicitly (per-root `TF_DATA_DIR`,
job-private `TF_PLUGIN_CACHE_DIR`, generated `TF_CLI_CONFIG_FILE`).
Deterministic efficiency gates 1–7 + measurement plan
(budgets 120 s warm critical path / 10 s warm plan; ≥3 cold + ≥5 warm
samples; matrix incl. corrupt-cache/mixed/fork; 1/10/100-root synthetic
scaling) are adopted as stated.

### 4.5 Workflow/report adoption (WS5)

One job per validation root (`tofu-<root>`, display `OpenTofu — <root>`);
fmt/init/validate as same-job ordered steps (init→validate `Data` edge);
tofu fmt once per scope, init once per root per attempt;
`MatrixEntry::derive("tofu", …)`;
`required.needs` ⊇ {plan, tofu jobs, lint}. Reports flow through
`write-task-report-v1` → exact-name fetch → `merge-v1` unchanged; reuse
disabled for tofu (`ReusedFromTaskCache` already fails closed); docs-only =
explicit `covered`/no-obligation disposition, never `Executed`; `NoWork`
never passes. No second verdict path. Validators: staged actionlint→
shellcheck→zizmor→shellcheck-`run:`-bodies; tofu steps as single-line
scalars; no `setup-opentofu` (Mise only). Read-only `plan`/`generate`
proven by before/after snapshots (no `.terraform`/lock/tool/source mutation,
no init/network during discovery). Verification detail is owned by
`rust-quality-contract.md` §9; per-root jobs and tofu step order are the
adoption delta.

### 4.6 E2E qualification (WS7 Q0–Q7)

Freeze identities (Q0); exact-source artifact acquisition with
download+sha256+`--version` proof, never relabel seed6 (Q1); real-binary
matrix E1–E5 with the qualified binary via the candidate generator (Q2);
old-to-new parity with obligation table + preview diffs + grep transcripts
(Q3); negatives N1–N8 must red Required with recorded cause (Q4); atomic
cutover checklist (Q5); PR-then-main CI verification on exact SHAs (Q6);
final ledger + summary with every limitation (Q7).

## 5. Verification

Spec §10 matrix + local gates through pinned tooling (`scripts/verify-local.sh`
offline subset; full: fmt, clippy, nextest, doctests, `alint
validate-config` + `alint check --fail-on-warning`, `cargo deny check
--locked`, `scripts/check-freshness.sh`, arch/size/schema tests,
actionlint/shellcheck/zizmor, Gates 0–8, real-binary tofu tests).
Phase-A refactor gates: T04 golden capture (plans, IR/YAML, task/cache IDs,
reports, fixture results over `minimal-cargo`, `nested`, `mbx-nextest`,
`parity`, …) re-run byte-identical after EVERY ownership move.

## 6. Source register addenda (beyond spec R01–R21/E01–E10/U01)

- W1–W7 workstream reports (`/tmp/velnor-ws{1,2,2b,3,4,5,6,7}-*.md`,
  re-verified at live head; WS2/WS2b snapshots predated BR-26).
- Q1 OpenTofu v1.13.1 release + SHA256SUMS
  (`https://github.com/opentofu/opentofu/releases/tag/v1.13.1`).
- Q2 `tofu_1.13.1_linux_amd64.tar.gz` sha256 `378ada19…` (aqua backend proof:
  consumer `mise.lock`).
- L1 producer main green @106bfd7 (run 36916298152); L2 consumer main red
  @f0eedf3 (run 36889650178, `no_work` fail-closed).
