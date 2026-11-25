# Deferral triage (review threads and Deferred issues)

Source: state/inv/deferred-threads.tsv (234 threads, 66 PRs) and issues #2816 #2818 #2821 #2849 #2853. Code read at origin/main 9394ee7399. Rule applied: never defer; fix or answer with evidence.

## Counts

- Fixed (reply says fixed, or reply cites a fix commit/evidence): 77 (72 flagged plus 5 whose reply says 'fixed in <sha>' or shows the red/green run: #2633 x4, #2579 x1)
- Refuted with evidence: 0 found (no reply argued the finding was wrong; refusals were all 'deferred')
- Owner-PR (open PRs, not fixed): 36
- Fixed on main: 44 (includes 3 threads on scripts/pseudonym-consistency.py, a file that does not exist on main)
- Still open: 49 code or lint items in 6 batches (incl. issues #2818 #2821 #2849 #2853, #2816 handled separately) plus 31 evidence-request threads that need an answer, not code
- Unverified marks: items whose current code I could not prove either way are listed still-open and say 'not shown handled'; a batch author must re-read the file first.

## Still-open batches (each one PR, 400 lines or fewer)

### 1. setup-velnor (add to existing batch with #2853) (4 items)

- #2779 `actions/setup-velnor/action.yml`: Revalidate the entry before deleting it - same race as #2853 (clear_entry), already in the batch ([thread](https://github.com/tailrocks/velnor-new/pull/2779#discussion_r4686623344))
- #2812 `actions/setup-velnor/action.yml`: Keep live evictions protected beyond the timeout - quarantine eviction is still protected only by the 3600 s timeout (action.yml `drop_abandoned_evictions`) ([thread](https://github.com/tailrocks/velnor-new/pull/2812#discussion_r4677665181))
- #2812 `actions/setup-velnor/action.yml`: Remove the legacy quarantine compatibility path - `.velnor-ci.evict.*` legacy path is still handled (action.yml line ~102) ([thread](https://github.com/tailrocks/velnor-new/pull/2812#discussion_r4677976636))
- #2818 (issue): no CI job runs scripts/test-setup-velnor-action.py: add mise task, [[lint]] job, regenerate ci.yml, decide on busybox leg (note: ci.yml is generated, so this batch touches generated output)

### 2. velnor-ci audit (inputs build invocation and units) (9 items)

- #2397 `crates/velnor-ci/src/io/units/workspace.rs`: Keep Cargo's live path after comparing directory identity - Workspace stores `physical(...)` canonical paths (units/workspace.rs From<Printed>), not Cargo's live path ([thread](https://github.com/tailrocks/velnor-new/pull/2397#discussion_r4672156687))
- #2397 `crates/velnor-ci/src/io/units/metadata.rs`: Handle CARGO_BUILD_TARGET_DIR as a target override - metadata.rs handles CARGO_TARGET_DIR and CARGO_BUILD_BUILD_DIR only; CARGO_BUILD_TARGET_DIR absent ([thread](https://github.com/tailrocks/velnor-new/pull/2397#discussion_r4672404388))
- #2397 `crates/velnor-ci/src/io/units/metadata.rs`: Keep inactive groups out of step-relative normalization - metadata.rs `step.join` applied when `invocation.is_none()`; inactive groups not shown to be excluded (needs a test) ([thread](https://github.com/tailrocks/velnor-new/pull/2397#discussion_r4672569440))
- #2397 `crates/velnor-ci/src/io/audit/build.rs`: Preserve explicit Cargo toolchains in the baseline probe - audit/build.rs MetadataInvocation::at uses plain `cargo`, no explicit toolchain ([thread](https://github.com/tailrocks/velnor-new/pull/2397#discussion_r4672573109))
- #2501 `crates/velnor-ci/src/io/audit/build/invocation.rs`: Keep targetless packages unselected under plural filters - selection.rs: `--workspace --exclude` selects members without proving they have a target ([thread](https://github.com/tailrocks/velnor-new/pull/2501#discussion_r4672188921))
- #2501 `crates/velnor-ci/src/io/audit/build/invocation/selection.rs`: Refuse plan-only Cargo invocations before invalidating - no refusal of plan-only invocations (--unit-graph, --build-plan, --print) in audit/build/invocation ([thread](https://github.com/tailrocks/velnor-new/pull/2501#discussion_r4673519428))
- #2501 `crates/velnor-ci/src/io/audit/build/invocation/selection.rs`: Prove selected packages have an enabled target - selection.rs: selected packages not proven to have an enabled target ([thread](https://github.com/tailrocks/velnor-new/pull/2501#discussion_r4673520285))
- #2506 `crates/velnor-ci/src/io/audit/build/invocation/package_spec.rs`: Decode file-URL paths before comparing package sources - package_spec.rs `located` compares the file URL raw, no percent-decoding ([thread](https://github.com/tailrocks/velnor-new/pull/2506#discussion_r4676673022))
- #2802 `crates/velnor-ci/src/io/audit/build/invocation.rs`: Include enabled optional dependencies in run selection - selection.rs `built()` skips every `dependency.optional`; enabled optional deps not included ([thread](https://github.com/tailrocks/velnor-new/pull/2802#discussion_r4677434064))

### 3. velnor-ci cache, credentials and private caches (7 items)

- #2508 `crates/velnor-ci/src/io/private_repo.rs`: Revalidate privacy before publishing private caches - revalidate privacy right before publishing private caches (open #2713) ([thread](https://github.com/tailrocks/velnor-new/pull/2508#discussion_r4651071445))
- #2834 `crates/velnor-render/src/private_steps.rs`: Recheck repository privacy before the Sources save - recheck repository privacy before the Sources save (open #2713) ([thread](https://github.com/tailrocks/velnor-new/pull/2834#discussion_r4680771258))
- #2615 `crates/velnor-ci/src/io/cache_credentials.rs`: Recognize linked-worktree Git directories before pruning - cache_credentials.rs: linked-worktree git dirs; only `config.worktree` is listed, not shown handled ([thread](https://github.com/tailrocks/velnor-new/pull/2615#discussion_r4670860623))
- #2615 `crates/velnor-ci/src/io/cache_credentials.rs`: Keep ref object trees out of repository detection - cache_credentials.rs: ref object trees in repository detection, not shown handled ([thread](https://github.com/tailrocks/velnor-new/pull/2615#discussion_r4670862886))
- #2615 `crates/velnor-ci/src/io/cache_credentials.rs`: Scan symbolic refs retained under hook-named ref directories - cache_credentials.rs: symbolic refs under hook-named ref dirs, not shown handled ([thread](https://github.com/tailrocks/velnor-new/pull/2615#discussion_r4672006930))
- #2615 `crates/velnor-ci/src/io/cache_credentials.rs`: Respect filesystem case rules when recognizing refs - cache_credentials.rs: case rules use `eq_ignore_ascii_case` on every filesystem ([thread](https://github.com/tailrocks/velnor-new/pull/2615#discussion_r4672266573))
- #2793 `crates/velnor-ci/src/io/sources.rs`: Remove the duplicate source-cache walk on misses - io/sources.rs: `cargo fetch` per root plus `cache_credentials::leak()`; duplicate cache walk on a miss not shown removed ([thread](https://github.com/tailrocks/velnor-new/pull/2793#discussion_r4676340551))

### 4. velnor-ci image build/publish and helpers (#1450, #2821) (8 items)

- #2808 `crates/velnor-ci/src/cli_args.rs`: Complete the builder migration in this change - builder migration incomplete, tracked by open #1450 ([thread](https://github.com/tailrocks/velnor-new/pull/2808#discussion_r4677529609))
- #2826 `crates/velnor-ci/src/image_publish/checks.rs`: Fetch the candidate from the successful CI run - image_publish/checks.rs: fetch the candidate from the successful CI run (open #1450) ([thread](https://github.com/tailrocks/velnor-new/pull/2826#discussion_r4679201866))
- #2826 `crates/velnor-ci/src/io/image_candidate/publish/flow.rs`: Drop the source checkout requirement from publishing - image_candidate/publish/flow.rs still requires a source checkout (open #1450) ([thread](https://github.com/tailrocks/velnor-new/pull/2826#discussion_r4679305162))
- #2838 `crates/velnor-ci/src/io/image_candidate/build.rs`: Execute the verified Docker inode, not a mutable path - image_candidate/build.rs executes by path, not the verified Docker inode ([thread](https://github.com/tailrocks/velnor-new/pull/2838#discussion_r4682458143))
- #2821 (issue): image build must refuse to start with credentials in its own environment (/proc/$PPID/environ); note #2823 refuses secret-named variables, so re-check whether this is now fixed-on-main before starting
- #2739 `crates/velnor-ci/src/plan_request/diff/changes.rs`: Preserve precise selection on pre-2.42 Git - plan_request/diff/changes.rs uses `git cat-file -Z --batch` (needs git 2.42), no pre-2.42 fallback ([thread](https://github.com/tailrocks/velnor-new/pull/2739#discussion_r4670869354))
- #2790 `crates/velnor-ci/src/io/early_list_child.rs`: Keep cancellation active through listing parsing - early_list_child.rs: cancellation during listing parsing, not shown handled ([thread](https://github.com/tailrocks/velnor-new/pull/2790#discussion_r4676882575))
- #2767 `crates/velnor-runner-host/src/journal/schema/clock_tick.rs`: Validate only predecessor guards before migration - runner-host journal/schema/clock_tick.rs migrate_v60_to_v61 validates the whole schema, not only predecessor guards ([thread](https://github.com/tailrocks/velnor-new/pull/2767#discussion_r4674317572))

### 5. velnor-render / velnor-rust / generator (7 items)

- #2394 `crates/velnor-render/src/install_steps.rs`: Reuse the generator build instead of rebuilding every run - generated-check from_source rebuilds the generator on every run (open #2440) ([thread](https://github.com/tailrocks/velnor-new/pull/2394#discussion_r4648051717))
- #2759 `crates/velnor-rust/src/ansible.rs`: Avoid installing the Ansible tools twice - ansible.rs has 'Install Ansible' and 'Install ansible-core and ansible-lint' steps; confirm the tools are not installed twice ([thread](https://github.com/tailrocks/velnor-new/pull/2759#discussion_r4673938687))
- #2798 `crates/velnor-rust/src/lint_inputs.rs`: Avoid running generated-check for task content edits - lint_inputs.rs: generated-check runs for mise task content edits ([thread](https://github.com/tailrocks/velnor-new/pull/2798#discussion_r4676685320))
- #2798 `generator/CONFIG.md`: Require a helper release containing the new selector - generator/CONFIG.md: state the helper release that carries the new Markdown selector ([thread](https://github.com/tailrocks/velnor-new/pull/2798#discussion_r4676684723))
- #2784 `generator/samples/qualification/view/.velnor/workflows.toml`: Align sample MBX pins with their workflows - sample workflows.toml MBX pins vs their workflows ([thread](https://github.com/tailrocks/velnor-new/pull/2784#discussion_r4676204415))
- #2833 `.velnor/rust-ci.toml`: Avoid compiling the generator on every pull request - generated-check compiles the generator on every PR (open #2048) ([thread](https://github.com/tailrocks/velnor-new/pull/2833#discussion_r4686895412))
- #2729 `.github/workflows/repository-checks.yml`: Update the scoped workflow ownership rules - repository-checks.yml header: scoped workflow ownership rules ([thread](https://github.com/tailrocks/velnor-new/pull/2729#discussion_r4667553336))

### 6. scripts/lints and CI gates (14 items)

- #2464 `.alint.yml`: Pin exempt evidence before bypassing the guard - .alint.yml: pin exempt evidence before bypassing the guard ([thread](https://github.com/tailrocks/velnor-new/pull/2464#discussion_r4673238615))
- #2464 `.alint.yml`: Reject mounted Windows profile paths - .alint.yml: reject mounted Windows profile paths (/mnt/c/Users) ([thread](https://github.com/tailrocks/velnor-new/pull/2464#discussion_r4673311317))
- #2464 `.alint.yml`: Cover /var/tmp scratch paths - .alint.yml: /var/tmp scratch paths (no-local-temp-paths); partly covered, confirm ([thread](https://github.com/tailrocks/velnor-new/pull/2464#discussion_r4673312543))
- #2464 `docs/implemented/freshness-evidence-2026-10-05.md`: Render redaction placeholders as literal text - docs/implemented/freshness-evidence-2026-10-05.md: render redaction placeholders as literal text ([thread](https://github.com/tailrocks/velnor-new/pull/2464#discussion_r4673313866))
- #2699 `scripts/private-names-guard.py`: ### 🛡️ Codex Security Review · _Automatically triggered_  **<sub><sub>![P3 Badge](https://img.shields.io/ba - OWNER DECISION: keep private-name verifier hashes outside the repo (public salt, offline oracle) ([thread](https://github.com/tailrocks/velnor-new/pull/2699#discussion_r4654670072))
- #2699 `scripts/private-names-guard.py`: ### 🛡️ Codex Security Review · _Automatically triggered_  **<sub><sub>![P2 Badge](https://img.shields.io/ba - private-names-guard.py: CHUNK splits on `*`, so bold-split names evade the match ([thread](https://github.com/tailrocks/velnor-new/pull/2699#discussion_r4672204393))
- #2699 `scripts/alint-line-cap-probe.sh`: Run the privacy gate from trusted policy - alint-line-cap-probe.sh runs the privacy gate from the PR checkout, not trusted policy ([thread](https://github.com/tailrocks/velnor-new/pull/2699#discussion_r4672205466))
- #2699 `scripts/alint-line-cap-probe.sh`: Install the pinned Python for the Alint probe - alint-line-cap-probe.sh: install pinned Python for the probe ([thread](https://github.com/tailrocks/velnor-new/pull/2699#discussion_r4672206501))
- #2699 `scripts/private-names-guard.py`: Bound matching-line collection to the report limit - private-names-guard.py: bound matching-line collection to the report limit ([thread](https://github.com/tailrocks/velnor-new/pull/2699#discussion_r4672210800))
- #2699 `scripts/private-names-guard.py`: Scan the contents of Git LFS objects - private-names-guard.py: Git LFS object contents are not scanned ([thread](https://github.com/tailrocks/velnor-new/pull/2699#discussion_r4672335215))
- #2699 `scripts/private-names-guard.py`: Stream blobs instead of retaining the whole repository - private-names-guard.py `blobs()` reads every blob in one `cat-file --batch` buffer; stream them ([thread](https://github.com/tailrocks/velnor-new/pull/2699#discussion_r4672336207))
- #2782 `.github/workflows/repository-checks.yml`: Wire the helper gate into the required merge verdict - repository-checks.yml: wire the helper gate into the required merge verdict (open #2773) ([thread](https://github.com/tailrocks/velnor-new/pull/2782#discussion_r4676210634))
- #2782 `.github/workflows/repository-checks.yml`: Serialize tag publication with queue admission - repository-checks.yml: serialize tag publication with queue admission (open #2773) ([thread](https://github.com/tailrocks/velnor-new/pull/2782#discussion_r4676274762))
- #2849 (issue): check-helper-gates.sh: parse destination from an unambiguous diff field (line 164) and lex char literals like '"' (line 124); two self-tests

### 7. Owner decision, no batch (1 issue)

- #2816: App private keys are job secrets of jobs that run repository code. The issue's own 'Done when' needs the owner's answer on promise 4 of docs/goals.md before any design; not fixed on main (generator/CONFIG.md still documents the residual).

### 8. Evidence-request threads, answer on the PR, no code (31 items)

Reviewer asked for independent test review, red/green proof, performance or final-head CI evidence. Under the new rule each needs a reply with evidence (run the test red on the parent, green on main; link the CI run) or a reasoned refusal; none changes code. Content checked only by title.

- #2615 `crates/velnor-ci/src/io/cache_credentials.rs`: Provide the required performance proof for recursive pruning ([thread](https://github.com/tailrocks/velnor-new/pull/2615#discussion_r4670863962))
- #2716 `crates/velnor-gen/tests/it/step_refs_shapes.rs`: Prove the newly added sweep cases red and green ([thread](https://github.com/tailrocks/velnor-new/pull/2716#discussion_r4670889379))
- #2739 `crates/velnor-ci/src/plan_request/diff/changes.rs`: Supply the mandatory final-head selection evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2739#discussion_r4671572929))
- #2740 `crates/velnor-ci/src/io/mbx_reports.rs`: Provide the required performance evidence for the byte cap ([thread](https://github.com/tailrocks/velnor-new/pull/2740#discussion_r4670831005))
- #2740 `crates/velnor-ci/tests/it/io_report/mbx_reports_sessions.rs`: Obtain an independent review of the new test plan ([thread](https://github.com/tailrocks/velnor-new/pull/2740#discussion_r4670832242))
- #2747 `crates/velnor-ci/src/mbx_store.rs`: Provide required cold-to-warm cache evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2747#discussion_r4673596117))
- #2755 `crates/velnor-ci/tests/it/cache_purge/if_public.rs`: **<sub><sub>![P1 Badge](https://img.shields.io/badge/P1-orange?style=flat)</sub></sub>  Obtain independent tes ([thread](https://github.com/tailrocks/velnor-new/pull/2755#discussion_r4672219653))
- #2770 `crates/velnor-model/src/step_setup.rs`: Publish CI-performance evidence for the reordered setup ([thread](https://github.com/tailrocks/velnor-new/pull/2770#discussion_r4674443254))
- #2770 `crates/velnor-gen/tests/it/step_refs_shapes.rs`: Obtain independent review for the regression test ([thread](https://github.com/tailrocks/velnor-new/pull/2770#discussion_r4674443832))
- #2779 `scripts/test-setup-velnor-action.py`: Obtain independent review before keeping the new tests ([thread](https://github.com/tailrocks/velnor-new/pull/2779#discussion_r4676206570))
- #2782 `.github/workflows/repository-checks.yml`: Supply the required CI performance evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2782#discussion_r4676211518))
- #2783 `crates/velnor-test-support/src/mise_trust.rs`: Obtain independent red-green verification for the test ([thread](https://github.com/tailrocks/velnor-new/pull/2783#discussion_r4676292695))
- #2783 `crates/velnor-test-support/src/mise_trust.rs`: Provide the required existing-engine CI evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2783#discussion_r4676293608))
- #2784 `mise.toml`: Prove the MBX cache lifecycle before upgrading ([thread](https://github.com/tailrocks/velnor-new/pull/2784#discussion_r4676203614))
- #2785 `crates/velnor-rust/src/report.rs`: Supply the required CI-performance evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2785#discussion_r4676230463))
- #2788 `crates/velnor-ci/src/io/audit/inventory/tests/root.rs`: Exercise root rejection through a public API ([thread](https://github.com/tailrocks/velnor-new/pull/2788#discussion_r4676251196))
- #2788 `crates/velnor-ci/src/io/audit/inventory/tests/root.rs`: Prove the new tests fail on broken code ([thread](https://github.com/tailrocks/velnor-new/pull/2788#discussion_r4676251981))
- #2792 `crates/velnor-ci/src/io/audit/inventory/tests/settled.rs`: Exercise the race through the public audit API ([thread](https://github.com/tailrocks/velnor-new/pull/2792#discussion_r4676389806))
- #2792 `crates/velnor-ci/src/io/audit/inventory/tests.rs`: Do not retain tests that were never observed red ([thread](https://github.com/tailrocks/velnor-new/pull/2792#discussion_r4676390551))
- #2795 `crates/velnor-ci/src/io/audit/inventory/tests.rs`: Exercise the race through the production scan API ([thread](https://github.com/tailrocks/velnor-new/pull/2795#discussion_r4676369818))
- #2795 `crates/velnor-ci/src/io/audit/inventory/tests.rs`: Provide the required independent test verification ([thread](https://github.com/tailrocks/velnor-new/pull/2795#discussion_r4676370534))
- #2796 `crates/velnor-ci/src/tools_local/store.rs`: Exercise the race through the public store API ([thread](https://github.com/tailrocks/velnor-new/pull/2796#discussion_r4676465287))
- #2799 `crates/velnor-ci/src/io/units.rs`: Provide the required performance evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2799#discussion_r4676943206))
- #2802 `crates/velnor-ci/src/io/audit/build/invocation.rs`: Supply performance evidence for the added preflight ([thread](https://github.com/tailrocks/velnor-new/pull/2802#discussion_r4676940538))
- #2812 `scripts/test-setup-velnor-action.py`: Review the test plan before drafting the race test ([thread](https://github.com/tailrocks/velnor-new/pull/2812#discussion_r4677977916))
- #2823 `crates/velnor-ci/tests/it/image_candidate/build_secrets.rs`: Obtain independent review and verification for the new tests ([thread](https://github.com/tailrocks/velnor-new/pull/2823#discussion_r4678890178))
- #2828 `.github/workflows/ci.yml`: Supply final-head CI performance evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2828#discussion_r4680262614))
- #2830 `Cargo.toml`: Supply the required CI performance evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2830#discussion_r4680328260))
- #2834 `crates/velnor-gen/tests/it/private_fill_roots.rs`: Obtain the required independent test-plan review ([thread](https://github.com/tailrocks/velnor-new/pull/2834#discussion_r4680768925))
- #2834 `crates/velnor-render/src/private_steps.rs`: Provide measured evidence for the expanded fetch ([thread](https://github.com/tailrocks/velnor-new/pull/2834#discussion_r4680770128))
- #2837 `crates/velnor-render/src/private_steps/fetch_step.rs`: Run the complete scoped generator verification ([thread](https://github.com/tailrocks/velnor-new/pull/2837#discussion_r4682005070))

## Fixed on main (merged PR deferrals now addressed)

- #2397 `crates/velnor-ci/src/io/units.rs`: Normalize relative build dirs during invocation validation - #2397 (41ee460e35) reads a relative CARGO_BUILD_BUILD_DIR from the step; units/metadata.rs `step.join(value)` ([thread](https://github.com/tailrocks/velnor-new/pull/2397#discussion_r4648015155))
- #2397 `crates/velnor-ci/src/io/units.rs`: Reject hashed dep-info at the target root - units/owner_tests.rs `a_hashed_dep_info_file_at_the_target_root_is_an_output_no_unit_owns` (#2799) ([thread](https://github.com/tailrocks/velnor-new/pull/2397#discussion_r4670299370))
- #2397 `crates/velnor-ci/src/io/audit/build/invocation.rs`: Give every compiling subcommand compiler handling - #2802 (75f9193d87) replays cargo run/bench/rustc/rustdoc in audit/build/invocation.rs ([thread](https://github.com/tailrocks/velnor-new/pull/2397#discussion_r4670300974))
- #2397 `crates/velnor-ci/src/io/units.rs`: Exempt generated files below nested OUT_DIR folders - units/built.rs `script_out_dir` walks `ancestors()` (nested OUT_DIR) (#2799) ([thread](https://github.com/tailrocks/velnor-new/pull/2397#discussion_r4670302096))
- #2397 `crates/velnor-ci/src/io/audit/build/invocation.rs`: **<sub><sub>![P2 Badge](https://img.shields.io/badge/P2-yellow?style=flat)</sub></sub>  Validate inactive work - invocation.validate_targets takes `group_dirs(...)` (audit/build.rs) ([thread](https://github.com/tailrocks/velnor-new/pull/2397#discussion_r4672570440))
- #2397 `crates/velnor-ci/src/io/audit/build/invocation.rs`: **<sub><sub>![P1 Badge](https://img.shields.io/badge/P1-orange?style=flat)</sub></sub>  Expand Cargo aliases b - audit/build/invocation/alias.rs resolves aliases before the build decision (#2802) ([thread](https://github.com/tailrocks/velnor-new/pull/2397#discussion_r4672682658))
- #2501 `crates/velnor-ci/src/io/audit/build/invocation.rs`: Treat doctests as narrowing workspace selection - invocation.rs `runs_doctests` keeps doctest runs out of narrowing (#2802) ([thread](https://github.com/tailrocks/velnor-new/pull/2501#discussion_r4672189983))
- #2501 `crates/velnor-ci/src/io/audit/build/invocation/selection.rs`: Keep excluded members selected when required as dependencies - selection.rs `built()` walks dependencies of chosen members regardless of `--exclude` ([thread](https://github.com/tailrocks/velnor-new/pull/2501#discussion_r4673521113))
- #2615 `crates/velnor-ci/src/io/cache_credentials.rs`: Prevent hook contents from disabling pruning - cache_credentials.rs LEFT_OUT_TREES + entry budget refuses the save (tests.rs hooks tests) (#2615 follow-ups) ([thread](https://github.com/tailrocks/velnor-new/pull/2615#discussion_r4670858101))
- #2615 `crates/velnor-ci/src/io/cache_credentials.rs`: Bind ordinary traversal to no-follow directory handles - cache_credentials/prune.rs uses O_NOFOLLOW + fstatat AT_SYMLINK_NOFOLLOW ([thread](https://github.com/tailrocks/velnor-new/pull/2615#discussion_r4670861798))
- #2615 `crates/velnor-ci/src/io/cache_credentials.rs`: Scrub sources after the final fetch - #2793 (f4b81243c6) scrubs sources after the fetch; #2748 closed ([thread](https://github.com/tailrocks/velnor-new/pull/2615#discussion_r4671424167))
- #2650 `crates/velnor-ci/src/bounded.rs`: Kill the process group on post-exit oversized output - #2827 (da0b0386d7) bounded grace after the supervised command exits ([thread](https://github.com/tailrocks/velnor-new/pull/2650#discussion_r4671955802))
- #2729 `crates/velnor-rust/src/plan.rs`: Omit the workflow when no stack supplies a job - #2759 (39d59f4972) closed the review findings of #2729 ([thread](https://github.com/tailrocks/velnor-new/pull/2729#discussion_r4667544340))
- #2729 `crates/velnor-rust/src/plan.rs`: Keep a planner for input-gated non-Rust lints - #2759 (39d59f4972) ([thread](https://github.com/tailrocks/velnor-new/pull/2729#discussion_r4667545587))
- #2729 `crates/velnor-rust/src/ansible.rs`: Install the declared Ansible collections - velnor-rust/src/ansible.rs 'Install collections' step runs `ansible.cli.galaxy collection install -r` ([thread](https://github.com/tailrocks/velnor-new/pull/2729#discussion_r4667547210))
- #2729 `crates/velnor-rust/src/ansible.rs`: Gate the Ansible job on affected inputs - ansible/select.rs `ansible_runs` gates the job on affected inputs (#2759, #2772) ([thread](https://github.com/tailrocks/velnor-new/pull/2729#discussion_r4667548806))
- #2729 `crates/velnor-rust/src/plan.rs`: Preserve the required verdict for non-Rust repositories - #2759 (39d59f4972) ([thread](https://github.com/tailrocks/velnor-new/pull/2729#discussion_r4667550236))
- #2729 `crates/velnor-rust/src/ansible.rs`: Quote playbook paths in generated commands - ansible.rs `bash_single` quotes each playbook path ([thread](https://github.com/tailrocks/velnor-new/pull/2729#discussion_r4667551978))
- #2729 `crates/velnor-rust/src/mise.rs`: Limit lint-only validation to installed tools - lint_jobs.rs `lint_only_tools` limits lint-only validation to installed tools ([thread](https://github.com/tailrocks/velnor-new/pull/2729#discussion_r4667554529))
- #2729 `crates/velnor-rust/src/ansible.rs`: Derive the play mapping indentation - ansible.rs derives play indent (`key_indent`) instead of fixing it ([thread](https://github.com/tailrocks/velnor-new/pull/2729#discussion_r4667555848))
- #2729 `.github/workflows/ci.yml`: Ignore the renamed generated workflow in Renovate - renovate.json ignores `.github/workflows/ci.yml` ([thread](https://github.com/tailrocks/velnor-new/pull/2729#discussion_r4667557358))
- #2729 `crates/velnor-rust/src/ansible.rs`: Put the installed Ansible CLIs on PATH - ansible.rs PREFIX puts `$RUNNER_TEMP/ansible/bin` on PATH ([thread](https://github.com/tailrocks/velnor-new/pull/2729#discussion_r4667558867))
- #2755 `crates/velnor-rust/src/base_cache.rs`: Keep the purge after the last Rust root is removed - #2785 (afecfa838a) keeps the private-cache purge job with no Cargo root ([thread](https://github.com/tailrocks/velnor-new/pull/2755#discussion_r4672463671))
- #2759 `crates/velnor-rust/src/ansible.rs`: Isolate Ansible imports from the checkout - ansible.rs HOOKS installs empty sitecustomize and PREFIX sets a clean PYTHONPATH ([thread](https://github.com/tailrocks/velnor-new/pull/2759#discussion_r4673935121))
- #2759 `crates/velnor-rust/src/report.rs`: Apply merge-group skipping to present-only plans - #2759 (39d59f4972) ([thread](https://github.com/tailrocks/velnor-new/pull/2759#discussion_r4673935800))
- #2759 `crates/velnor-rust/src/lint_jobs.rs`: Validate tools for generated-check-only repositories - lint_jobs.rs `lint_only_tools` (#2759) ([thread](https://github.com/tailrocks/velnor-new/pull/2759#discussion_r4673936408))
- #2759 `crates/velnor-rust/src/ansible.rs`: Reject Ansible slug collisions for older helpers - ansible.rs `slug_collision` refuses a colliding lint slug ([thread](https://github.com/tailrocks/velnor-new/pull/2759#discussion_r4673937135))
- #2759 `crates/velnor-rust/src/ansible.rs`: Install collections into job-local storage - ansible.rs installs into `$RUNNER_TEMP/ansible` (job-local) ([thread](https://github.com/tailrocks/velnor-new/pull/2759#discussion_r4673937956))
- #2788 `crates/velnor-ci/src/io/audit/inventory/probe.rs`: Remove the remaining production callback seam - inventory/probe.rs defines the callback Probe only under cfg(test) (#2788) ([thread](https://github.com/tailrocks/velnor-new/pull/2788#discussion_r4676252762))
- #2796 `crates/velnor-ci/src/tools_local/store.rs`: Lock ordinary saves before accepting an existing entry - tools_local/store.rs `claim(...)` locks the key in the ordinary save path (#2796) ([thread](https://github.com/tailrocks/velnor-new/pull/2796#discussion_r4676467163))
- #2796 `crates/velnor-ci/src/tools_local/store.rs`: Make stale-key claims mutually exclusive - #2810 (87b17a5794) takes over a stale tools key lock atomically; #2800 closed ([thread](https://github.com/tailrocks/velnor-new/pull/2796#discussion_r4676468072))
- #2796 `crates/velnor-ci/src/tools_local/store.rs`: Do not retire entries with active restore leases - store.rs shared `lease` on entries; #2796 (d298fb3674) ([thread](https://github.com/tailrocks/velnor-new/pull/2796#discussion_r4676578745))
- #2796 `crates/velnor-ci/src/tools_local/store.rs`: Record candidate identities without following symlinks - store.rs uses `fs::symlink_metadata` for candidate identity ([thread](https://github.com/tailrocks/velnor-new/pull/2796#discussion_r4676579748))
- #2796 `crates/velnor-ci/src/tools_local/store.rs`: Bound entries skipped for active key claims - #2796 (d298fb3674); #2752 closed ([thread](https://github.com/tailrocks/velnor-new/pull/2796#discussion_r4676804332))
- #2808 `crates/velnor-ci/src/image_candidate/build.rs`: Pass the source SHA into the Dockerfile - #2824 (1423b6513d) passes VELNOR_SOURCE_SHA; #2811 closed ([thread](https://github.com/tailrocks/velnor-new/pull/2808#discussion_r4677534504))
- #2808 `crates/velnor-ci/src/io/image_candidate/output.rs`: Reject symlinks in every output-path component - image_candidate/output.rs `symlinked()` rejects a symlink in any ancestor ([thread](https://github.com/tailrocks/velnor-new/pull/2808#discussion_r4677536577))
- #2808 `crates/velnor-ci/src/io/early_list_child.rs`: Bound drains from children that escape the process group - #2827 (da0b0386d7) bounded grace whatever holds the output ([thread](https://github.com/tailrocks/velnor-new/pull/2808#discussion_r4678130368))
- #2808 `crates/velnor-ci/src/io/image_candidate/docker.rs`: Pin the Docker front-end before running validation - #2838 (b71a5bda5a) pins the Docker client by binary hash; #2820 closed ([thread](https://github.com/tailrocks/velnor-new/pull/2808#discussion_r4678343014))
- #2808 `crates/velnor-ci/src/io/image_candidate/build.rs`: ### 🛡️ Codex Security Review · _Automatically triggered_  **<sub><sub>![P2 Badge](https://img.shields.io/ba - #2838 (b71a5bda5a); #2820 closed ([thread](https://github.com/tailrocks/velnor-new/pull/2808#discussion_r4678448203))
- #2834 `crates/velnor-render/src/private_steps.rs`: Support quoted manifests in oversized private fetches - #2837 (18440c1489); #2836 closed ([thread](https://github.com/tailrocks/velnor-new/pull/2834#discussion_r4681278055))
- #2834 `crates/velnor-rust/src/private_fetch.rs`: Limit grouped PR fetches to selected members - #2837 (18440c1489); #2836 closed ([thread](https://github.com/tailrocks/velnor-new/pull/2834#discussion_r4681279539))
- #2699 `scripts/pseudonym-consistency.py`: Preserve parent record keys for nested repository objects - scripts/pseudonym-consistency.py is absent on origin/main ([thread](https://github.com/tailrocks/velnor-new/pull/2699#discussion_r4672209267))
- #2699 `scripts/pseudonym-consistency.py`: Compare accepted owner aliases across matching records - scripts/pseudonym-consistency.py is absent on origin/main ([thread](https://github.com/tailrocks/velnor-new/pull/2699#discussion_r4672212087))
- #2699 `scripts/pseudonym-consistency.py`: Parse quoted CSV record numbers structurally - scripts/pseudonym-consistency.py is absent on origin/main ([thread](https://github.com/tailrocks/velnor-new/pull/2699#discussion_r4672213351))

## Owner-PR (handled by the agents on those PRs)

### PR #2183: 2 threads

- #2183 `crates/velnor-ci/Cargo.toml`: Add regression coverage for corrected ZIP header bounds ([thread](https://github.com/tailrocks/velnor-new/pull/2183#discussion_r4642739173))
- #2183 `crates/velnor-ci/Cargo.toml`: Supply the required final-head CI evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2183#discussion_r4642739457))

### PR #2379 (being closed as superseded by #2448; verify its findings are covered there): 1 threads

- #2379 `.velnor/rust-ci.toml`: Measure the newly installed Python before merging ([thread](https://github.com/tailrocks/velnor-new/pull/2379#discussion_r4649996236))

### PR #2448: 6 threads

- #2448 `scripts/no-doctests.py`: Require a title before recognizing a Setext underline ([thread](https://github.com/tailrocks/velnor-new/pull/2448#discussion_r4649993697))
- #2448 `scripts/no-doctests.py`: Require non-whitespace content in footnote labels ([thread](https://github.com/tailrocks/velnor-new/pull/2448#discussion_r4649994051))
- #2448 `scripts/no-doctests.py`: Preserve Markdown semantics in mixed-style block docs ([thread](https://github.com/tailrocks/velnor-new/pull/2448#discussion_r4649994477))
- #2448 `scripts/no-doctests.py`: Do not concatenate literals from arbitrary bracketed macros ([thread](https://github.com/tailrocks/velnor-new/pull/2448#discussion_r4649994915))
- #2448 `scripts/check-no-doctests.sh`: Sanitize inherited rustdoc test arguments ([thread](https://github.com/tailrocks/velnor-new/pull/2448#discussion_r4670841591))
- #2448 `scripts/check-no-doctests.sh`: Filter procedural-macro stdout from doctest matches ([thread](https://github.com/tailrocks/velnor-new/pull/2448#discussion_r4670842697))

### PR #2525: 6 threads

- #2525 `scripts/check-helper-gates.sh`: Preserve dotted helper pins during literal masking ([thread](https://github.com/tailrocks/velnor-new/pull/2525#discussion_r4655009352))
- #2525 `scripts/check-helper-gates.sh`: Lex comments before accepting helper-gate markers ([thread](https://github.com/tailrocks/velnor-new/pull/2525#discussion_r4672307192))
- #2525 `scripts/check-helper-gates.sh`: Mask Rust raw strings before classifying constants ([thread](https://github.com/tailrocks/velnor-new/pull/2525#discussion_r4673044346))
- #2525 `scripts/check-helper-gates.sh`: Track multiline helper-version constants ([thread](https://github.com/tailrocks/velnor-new/pull/2525#discussion_r4673221688))
- #2525 `scripts/check-helper-gates.sh`: Parse the destination from an unambiguous diff field ([thread](https://github.com/tailrocks/velnor-new/pull/2525#discussion_r4685446821))
- #2525 `scripts/check-helper-gates.sh`: Lex character literals before masking strings ([thread](https://github.com/tailrocks/velnor-new/pull/2525#discussion_r4685448803))

### PR #2577: 1 threads

- #2577 `crates/velnor-ci/src/io/cache_delete_entry.rs`: Refuse candidates without proof of the restored version ([thread](https://github.com/tailrocks/velnor-new/pull/2577#discussion_r4655504497))

### PR #2815: 2 threads

- #2815 `crates/velnor-render/src/tools_steps.rs`: Verify and record the runner timeout contract ([thread](https://github.com/tailrocks/velnor-new/pull/2815#discussion_r4678031244))
- #2815 `crates/velnor-gen/tests/it/tools_local.rs`: Share the stale-lock threshold with the renderer ([thread](https://github.com/tailrocks/velnor-new/pull/2815#discussion_r4678031897))

### PR #2839: 18 threads

- #2839 `crates/velnor-render/src/image_publish.rs`: ### 🛡️ Codex Security Review · _Automatically triggered_  **<sub><sub>![P2 Badge](https://img.shields.io/ba ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4683010949))
- #2839 `crates/velnor-render/src/image_publish.rs`: ### 🛡️ Codex Security Review · _Automatically triggered_  **<sub><sub>![P2 Badge](https://img.shields.io/ba ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4683012305))
- #2839 `crates/velnor-render/src/image.rs`: Include Rust sources in the PR path filter ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4683013766))
- #2839 `crates/velnor-gen/src/image.rs`: Validate the actual required features ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4683015223))
- #2839 `crates/velnor-render/src/image_publish.rs`: Wait for SHA publication before promotion ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4683016675))
- #2839 `crates/velnor-render/src/image_publish.rs`: ### 🛡️ Codex Security Review · _Automatically triggered_  **<sub><sub>![P2 Badge](https://img.shields.io/ba ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4683730124))
- #2839 `crates/velnor-render/src/image_build.rs`: Gate each image build on its own inputs ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4683731696))
- #2839 `crates/velnor-render/src/image.rs`: Stop stale push runs from blocking the current image ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4683750567))
- #2839 `crates/velnor-render/src/image.rs`: Share identical compilation across image builds ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4683752030))
- #2839 `crates/velnor-render/src/image_publish.rs`: Recheck the branch tip after building the image ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4683941379))
- #2839 `crates/velnor-render/src/image_publish.rs`: Keep the polling gate off the image runner pool ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4683947486))
- #2839 `crates/velnor-render/src/image.rs`: Run image checks for bot-authored dependency updates ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4684832070))
- #2839 `crates/velnor-rust/src/config_image.rs`: Reject image inputs that are not tracked ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4684833392))
- #2839 `crates/velnor-render/src/image_release.rs`: ### 🛡️ Codex Security Review · _Automatically triggered_  **<sub><sub>![P1 Badge](https://img.shields.io/ba ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4684834720))
- #2839 `crates/velnor-render/src/image.rs`: Run image validation for merge queue commits ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4684836165))
- #2839 `crates/velnor-ci/src/gate.rs`: Preserve successful jobs from the gate's current workflow ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4684837747))
- #2839 `crates/velnor-render/src/image_publish.rs`: ### 🛡️ Codex Security Review · _Automatically triggered_  **<sub><sub>![P2 Badge](https://img.shields.io/ba ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4685006592))
- #2839 `crates/velnor-gen/src/image.rs`: Reject ghcr.io for repositories on custom GitHub domains ([thread](https://github.com/tailrocks/velnor-new/pull/2839#discussion_r4686852028))

## Fixed or answered (dropped)

72 rows flagged `reply_says_fixed=yes` across merged and open PRs (see the TSV), plus these whose reply cites the fix:
- #2633 `crates/velnor-ci/tests/it/check_metadata_gen.rs`: Honor the keep flag in wrapper Drop implementations - reply cites fix or evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2633#discussion_r4651623050))
- #2633 `crates/velnor-test-support/src/scratch.rs`: Clean up scratch trees left non-writable by panics - reply cites fix or evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2633#discussion_r4651623149))
- #2633 `crates/velnor-test-support/src/scratch.rs`: Fail when an existing scratch tree cannot be cleared - reply cites fix or evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2633#discussion_r4652143177))
- #2633 `crates/velnor-test-support/src/scratch.rs`: Supply a passing full relevant-suite run - reply cites fix or evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2633#discussion_r4653612487))
- #2579 `crates/velnor-gen/tests/it/step_refs_sweep.rs`: Obtain independent red-green verification for this test - reply cites fix or evidence ([thread](https://github.com/tailrocks/velnor-new/pull/2579#discussion_r4654923929))
