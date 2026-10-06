# Evidence index

Initial review date: 4 October 2026. Evidence refreshed at 4 October 2026, 17:54 UTC. Times in the data files are UTC.

This package records research and CI evidence. It does not prove that source changes were merged, products were released, or a host was updated. Check the later evidence entries for refreshed run results.

The evidence has four classes. **Observed** means a retrieved run, log, or GitHub record. **Source** means inspected source code. **Candidate** means a finding that needs a current-source or live test. **Proposed** means a requirement in this package. A branch's own evidence document is not an independent test result.

## Fixed identities

| Item | Identity |
| --- | --- |
| Consumer PR | ChainArgos/java-monorepo #2085 |
| Consumer branch | n1-qualification |
| Consumer head | 7cbe1db11ccabb463053a93a8a08d33bda418b29 |
| Consumer base | 0a937e0c0442782bfb88c43cfdf67c1fc3f3b4f0 |
| Tested synthetic merge | 364d8b2837a66e29333b7017388bb9ad90da469e |
| Initial CI capture | 37178675286, attempt 1; nonterminal snapshot |
| Refreshed CI run | 37178675286, attempt 2; completed failure (see E18) |
| Velnor main / generator source | 47815c83b9eeadbaf84b741918fffa7ea550da89 |
| Generator release | generator-47815c83b9eeadbaf84b741918fffa7ea550da89 |
| Linux generator asset SHA-256 | 60b0507eb6774e6f2bb42ea6d964dbf46215f09ecd514bca80a11315dcc22d6a |
| Official runner in inspected logs | 2.337.0 |
| Deployed native daemon and container image identities | Not independently established by this review |

The consumer PR description has older release wording. The runtime log is the source for the generator that actually ran. E18 records the completed attempt-2 refresh.

## E01 — Run and job inventory (Observed)

`https://github.com/ChainArgos/java-monorepo/pull/2085`

`https://github.com/ChainArgos/java-monorepo/actions/runs/37178675286`

`https://api.github.com/repos/ChainArgos/java-monorepo/actions/runs/37178675286/jobs?filter=latest&per_page=100`

The retrieved inventory contains 43 job records: 20 Rust pairs, an Actionlint pair, and Plan. The captured response has 36 successful jobs, two cancelled jobs, three queued jobs, and two running jobs. This is a **nonterminal snapshot**, not the final result. A later refresh can have additional completed or instantiated jobs. Each CSV row includes its job URL. Queued jobs had a placeholder start time and no runner. Their normalized start time is empty.

## E02 — Small matched workload (Observed)

Hosted AMQ: `https://github.com/ChainArgos/java-monorepo/actions/runs/37178675286/job/111366755448`

Scale Set AMQ: `https://github.com/ChainArgos/java-monorepo/actions/runs/37178675286/job/111366755460`

Both logs use the same tested merge and generator product. They show action preparation, a second consumer checkout, tool setup, source-cache results, MBX results, compile steps, tests, and post actions. Composite markers give phase durations. The action-repository interval also includes extraction and preparation. It is not a pure network-transfer measurement.

## E03 — Heavy hosted workload (Observed)

`https://github.com/ChainArgos/java-monorepo/actions/runs/37178675286/job/111366755645`

The eth-processor-app log shows Clippy, a development build, a separate test build, 318 tests across 18 binaries, three skipped tests, documentation, and disk-pressure collection. Test execution took 229.887 seconds. Do not confuse this value with compilation or total job duration.

## E04 — Branch and PR inventory (Observed)

`https://api.github.com/repos/tailrocks/velnor-new/branches?per_page=100`

`https://github.com/tailrocks/velnor-new/pulls`

The review found 20 branch refs. The branch CSV preserves every ref and a proposed disposition. #26 and #27 were already merged. #29 and #25 were open. #28 and #20 were draft work. These are review-time states.

## E05 — MBX repair carrier (Source)

`https://github.com/tailrocks/velnor-new/pull/29`

`https://github.com/tailrocks/velnor-new/blob/4ad1e34a1e20589386ddefdc25eaceb067341f0b/crates/velnor-actions-workflow-renderer/src/cache_steps.rs`

Inspected changes move MBX cache lifecycle to the native action. They remove the manual bundle implementation. They set hosted object-cache isolation and a job suffix. They qualify MBX 1.22.0 and action v1.7.1 in the branch. A local test claim in the PR is not a downstream cold/warm proof.

## E06 — Competing MBX branch (Source)

`https://github.com/tailrocks/velnor-new/compare/47815c83b9eeadbaf84b741918fffa7ea550da89...903fddef41170ea85e6a4dcbe7e2248bfa3f23b9`

`https://github.com/tailrocks/velnor-new/blob/903fddef41170ea85e6a4dcbe7e2248bfa3f23b9/crates/velnor-actions-workflow-renderer/src/cache_p08_http.rs`

The branch is ten commits ahead of the inspected main. It has diverged from #29, whose head has fifteen different commits. Its HTTP bootstrap and pin-validation work need semantic comparison. Do not apply two competing cache implementations.

## E07 — Worker and tar fixes (Source)

`https://github.com/tailrocks/velnor-new/pull/25`

`https://github.com/tailrocks/velnor-new/blob/63713b8074515c837fee14808385e6775745d731/crates/velnor-runner/crates/velnor-runner-host/src/launch/turn.rs`

The branch now includes worker release, capacity, journal, stage, and DinD changes. Its title does not describe the full patch set. The tar layer needs separate semantic tests. A successful archive download is not proof of correct cache extraction.

## E08 — Capacity candidate (Source / Candidate)

`https://github.com/tailrocks/velnor-new/blob/5ab06042fa171889650b2bfca164d59926448ddc/crates/velnor-runner/crates/velnor-runner-host/src/launch/pressure.rs`

Compared with #25, this branch is one commit ahead and seventeen commits behind. The inspected policy starts at one slot. macOS probes use host load, host memory, and `df` on `/`. Missing metrics leave the current slot count unchanged. Adapt the idea to the actual Docker virtual machine, storage location, and worker load. This review did not establish that this candidate is deployed.

## E09 — Performance branch and actual test-preparation fix (Source)

`https://github.com/tailrocks/velnor-new/blob/6b1632d4b6bdd39e348ca33ac701241cd987b921/crates/velnor-actions-rust/src/argv.rs`

The branch uses `nextest list --list-type binaries-only` for Nextest build preparation. It uses `cargo test --no-run` for Cargo test preparation. Preserve any distinct application-build obligation. The branch also contains selection work and performance collection. Extract small coherent fixes first.

## E10 — Audit documents (Candidate unless corroborated)

`https://github.com/tailrocks/velnor-new/blob/6b1632d4b6bdd39e348ca33ac701241cd987b921/docs/reviews/ci-performance-current-main-47815-source-audit-v2.md`

`https://github.com/tailrocks/velnor-new/blob/6b1632d4b6bdd39e348ca33ac701241cd987b921/docs/reviews/ci-performance-generator-audit.md`

`https://github.com/tailrocks/velnor-new/blob/6b1632d4b6bdd39e348ca33ac701241cd987b921/docs/reviews/ci-performance-completion-ledger.md`

The older generator audit uses an older base. Reproduce its findings on the selected integration head. The completion ledger is open. Source-only changes and unmatched timing claims are not deployment or performance evidence.

## E11 — Mise cache ownership (Source)

`https://github.com/tailrocks/velnor-new/blob/47815c83b9eeadbaf84b741918fffa7ea550da89/crates/velnor-actions-workflow-renderer/src/cache_steps_tools.rs`

`https://github.com/jdx/mise-action/blob/9149ea85001c7435d5a66bb127d6a1b6227cb0a5/src/index.ts`

The Velnor save path is `~/.local/share/mise`. The action restores its computed cache path. Rustup uses a different configured home in the logs. The action's built-in save registration is inside its install path. Merely changing `cache_save` while keeping `install: false` is not a complete repair.

## E12 — Official runner action archive cache (Source)

`https://github.com/actions/runner/blob/v2.337.0/src/Runner.Common/Constants.cs`

`https://github.com/actions/runner/blob/v2.337.0/src/Runner.Worker/ActionManager.cs`

Inspect action resolution and archive preparation around lines 1050–1405. The runner supports `ACTIONS_RUNNER_ACTION_ARCHIVE_CACHE`. Linux archives use the resolved action commit. The ordinary archive-cache path still copies, extracts, and copies the action tree. The optional unpacked-action path needs separate qualification. Keep authorization and job isolation intact.

## E13 — MBX action inputs (Source)

`https://github.com/jdx/mr-boxington-action/blob/v1.7.1/action.yml`

The inspected metadata supports explicit `toolchain`, object-cache isolation, and cache-key suffixes. Without `version`, the action can use MBX from PATH; it can also install latest when no suitable binary exists. A strict preflight is thus required before choosing the PATH mode. Same-repository PR saving is opt-in. Fork PR saving is not permitted by that option.

## E14 — GitHub cache identity (Primary documentation)

`https://github.com/actions/cache#cache-version`

The cache version depends on the path list and compression method, not only the displayed key. Use the cache API to compare actual key, version, scope, and bytes. Do not infer the complete cause of a cache miss from the key text alone.

## E15 — Git object reuse (Primary documentation)

`https://git-scm.com/docs/git-clone`

`--reference-if-able` can reuse local objects. `--dissociate` removes the resulting long-term object dependency. Mutable shared alternates can fail when the source performs garbage collection. These are Git options, not claimed inputs of actions/checkout.

## E16 — Emulation and test archives (Primary documentation)

`https://docs.docker.com/build/building/multi-platform/`

`https://nexte.st/docs/ci-features/archiving/`

Docker documents emulation costs. That does not establish the current host's CPU or emulator. Nextest archives can transport test binaries and metadata. They do not remove source, target, runtime, and dynamic-library compatibility requirements.

## E17 — Writing reference

`https://www.asd-ste100.org/faq.html`

The plan uses short active instructions and defined technical names. This package does not claim formal STE certification or a complete licensed-dictionary audit.

## E18 — Completed consumer attempt 2 refresh (Observed)

Refreshed from GitHub at `2026-10-04T17:54:41Z`. PR #2085 is open at head `7cbe1db11ccabb463053a93a8a08d33bda418b29`, base `0a937e0c0442782bfb88c43cfdf67c1fc3f3b4f0`, and synthetic merge `364d8b2837a66e29333b7017388bb9ad90da469e`. Run `37178675286` is attempt 2 and ended `failure`. The API lists 45 jobs: 40 passed, two failed, two cancelled, and one skipped. This is not a passing required run. The only run returned for this head is `37178675286`.

The Plan log acquired `velnor-actions-0.1.0` from release `generator-47815c83b9eeadbaf84b741918fffa7ea550da89`. Its Linux asset SHA-256 is `60b0507eb6774e6f2bb42ea6d964dbf46215f09ecd514bca80a11315dcc22d6a`; the log verifies it with `sha256sum -c`. The PR description instead names generator `d40868152f7fe0106e3ede858a411f502f00810f6`. Treat the log identity as the product used by this run. The official runner reports version 2.337.0. These job logs do not establish the installed daemon executable or runner and DinD image digests.

The four target Scale Set jobs use the same source checkout and generator release. Their queue and execution times are:

| Workload | Job | Queue | Job | Set up job | Result |
| --- | ---: | ---: | ---: | ---: | --- |
| tron-migration | 111444356886 | 3:02:27 | 20:45 | 2:10 | passed |
| eth-migration | 111444356580 | 3:25:10 | 21:22 | 4:07 | passed |
| legacy-grpc-server | 111444356812 | 1:13:36 | 27:27 | 2:29 | failed |
| eth-processor-app | 111444356757 | 1:24:37 | 30:32 | 6:44 | cancelled while compiling |

Queue time is job creation to worker start. Job time is worker start to completion. Set up time is the job's `Set up job` step. The phase CSV gives source and nesting details. The action archive interval includes transfer, extraction, and copy; it is not a pure download measurement.

The action setup downloaded the consumer repository as an action archive and later checked it out again. The archive intervals were 117.107 s for tron-migration, 231.665 s for eth-migration, 132.294 s for legacy-grpc-server, and 377.927 s for eth-processor-app. Later checkout markers were 18.992 s, 24.105 s, 23.414 s, and 32.803 s in the same order.

For tron-migration, the installed tools in the job were selected with `mise --no-config --no-env --no-hooks exec rust@1.98.1 mr-boxington@1.21.1 aqua:nextest-rs/nextest/cargo-nextest@0.9.146`. `Build test executables` ran `mbx build --locked --offline --manifest-path backend-rust/tron-migration/Cargo.toml --package tron-migration` in the development profile for 82.496 s. Nextest ran `mbx nextest run --profile ci --locked --offline --manifest-path backend-rust/tron-migration/Cargo.toml --package tron-migration --no-tests fail`, compiled test-profile binaries for about 595 s, then ran one test for 43.233 s. The Nextest step took 643.754 s. The job used `jdx/mr-boxington-action@1687e54eb349cadf61fa38b5813a77875489e8e6` with MBX 1.21.1 and cache generation `velnor-mbx-1.21.1`. Its log says `No mbx cache found`; PR mode says restore only because save-on-pull-request is off. The separate bundle restore also missed. The Nextest MBX report was 0 hits, 0 misses, 788 not looked up, and 9 bypassed; the local store held 928.7 MiB and the report listed 0 transport bytes. It states that all 788 compilations had no usable prior inputs or matching prediction for a lookup. The nine bypasses were eight `cc-missing-output` and one `cc-not-a-compile`. This confirms a cold MBX path in this job. It does not prove the cause of the prediction or profile mismatch.

Test counts from the job logs: tron-migration selected and ran one test; one passed, zero failed, and zero skipped. eth-migration selected and ran two tests; both passed, zero failed, and zero skipped. legacy-grpc-server selected and ran 76 tests; 74 passed, two failed, and zero skipped. It produced no successful suite result. `wallet_test::test_get_wallet_not_found` and `wallet_test::test_list_wallets_by_address` each failed with `WaitContainer(StartupTimeout)` after about 67.01 s and 68.79 s. The service agent owns the reproduction. The eth-processor-app job was cancelled at 30:32 while compilation was still active. The job selected-test count is unavailable. No test executed or produced a result before cancellation. The workflow source sets a 30-minute timeout and PR `cancel-in-progress`, but the job endpoint reports only `cancelled`. Check-run annotations were blocked by the API connector. No controller event or explicit timeout/cancel actor was available in this evidence set. The duration matches the configured timeout, but the cancellation cause is unconfirmed.

The hosted tron-migration job restored 147,446,399 bytes (about 141 MiB) from `actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9`. Both jobs used key `velnor-v1-sources-x86_64-unknown-linux-gnu-1.98.1-c02dd0eb97e52230ea2edecdd3c3795aef190164583a0ec0cf2f8b9007714341` and the same action version. The Scale Set job looked under `/home/runner/_work/_temp/velnor/cargo`; the hosted job used `/home/runner/work/_temp/velnor/cargo`. The hosted cache hit and Scale Set cache miss. This proves a path difference. The cache metadata endpoint was not available in this evidence set, so the key alone does not prove the cache-version or scope cause. `HOME`, the `RUNNER_TEMP` variable, effective `CARGO_HOME` and `RUSTUP_HOME`, and the resolved Rust symlink target were not logged.

Report artifacts are not product images. Attempt 2 artifacts and SHA-256 values are: Plan `11304202842`, `26680d48ab6a2f10fdc277f75a3f90a436b44033b46afecddec931e8ad66e387`; tron `11308768875`, `be46cb4c1eb6b1a4ae71bd016387abd9d554ef8107b1110a2d2a27c516a0e6a8`; eth migration `11309671251`, `f8110a3e3a9f4af9ff4c4e5f958e9645d02f95dc4e9c9ef8a36b5c0339b5fc00`; legacy `11306318122`, `e483f75e742da8cca4c04cd187901de128e4ecaacc36288df3b7f74967d373ee`; eth processor `11307046870`, `2422d4b0817779517ea8d235456be781759b165fb4ae71b1c9b745a651f5d0a8`.

At this refresh, checksum verification passed for the README, this index, and the phase CSV. The package-wide check found that `goal.md` did not match its recorded SHA-256. Its owner was editing it at the same time. This work left `goal.md` unchanged. Recheck the manifest after that edit ends.

Sources: [PR #2085](https://github.com/ChainArgos/java-monorepo/pull/2085), [attempt 2 jobs API](https://api.github.com/repos/ChainArgos/java-monorepo/actions/runs/37178675286/attempts/2/jobs?per_page=100), [Plan job](https://github.com/ChainArgos/java-monorepo/actions/runs/37178675286/job/111444113941), [tron-migration](https://github.com/ChainArgos/java-monorepo/actions/runs/37178675286/job/111444356886), [eth-migration](https://github.com/ChainArgos/java-monorepo/actions/runs/37178675286/job/111444356580), [legacy-grpc-server](https://github.com/ChainArgos/java-monorepo/actions/runs/37178675286/job/111444356812), [eth-processor-app](https://github.com/ChainArgos/java-monorepo/actions/runs/37178675286/job/111444356757), [hosted tron-migration](https://github.com/ChainArgos/java-monorepo/actions/runs/37178675286/job/111444356912).

Bounded raw excerpts from those logs:

```text
Plan 111444113941 at 2026-10-04T13:16:18Z: VELNOR_RELEASE_COMMIT=47815c83b9eeadbaf84b741918fffa7ea550da89
Plan 111444113941 at 2026-10-04T13:16:18Z: VELNOR_ASSET_SHA256=60b0507eb6774e6f2bb42ea6d964dbf46215f09ecd514bca80a11315dcc22d6a
Plan 111444113941 at 2026-10-04T13:16:19Z: /home/runner/work/_temp/velnor/bin/velnor-actions-0.1.0: OK
Tron 111444356886 at 2026-10-04T16:22:15Z: No mbx cache found
Tron 111444356886 at 2026-10-04T16:22:15Z: Restore only (pull request; save-on-pull-request is off)
Tron 111444356886: `mise --no-config --no-env --no-hooks exec rust@1.98.1 mr-boxington@1.21.1 aqua:nextest-rs/nextest/cargo-nextest@0.9.146 -- mbx build --locked --offline --manifest-path backend-rust/tron-migration/Cargo.toml --package tron-migration`
Tron 111444356886: `mise --no-config --no-env --no-hooks exec rust@1.98.1 mr-boxington@1.21.1 aqua:nextest-rs/nextest/cargo-nextest@0.9.146 -- mbx nextest run --profile ci --locked --offline --manifest-path backend-rust/tron-migration/Cargo.toml --package tron-migration --no-tests fail`
Tron 111444356886 at 2026-10-04T16:36:38Z: object cache: 0 hits, 0 misses, 788 not looked up, 9 bypassed; 0 B downloaded, 0 B uploaded, 928.7 MiB stored locally
Tron 111444356886 at 2026-10-04T16:36:38Z: 788 compilations had no usable prior inputs or matching prediction for a lookup
Tron 111444356886 at 2026-10-04T16:36:38Z: bypass reasons: 8 cc-missing-output, 1 cc-not-a-compile
Eth processor 111444356757 at 2026-10-04T15:11:27Z: ##[error]The operation was canceled.
```

## E19 — PR #39 merge and fresh-base source verification (Observed)

PR #39 merged as `96b08aa236a6b5afcbd6c768bfdc83e6fa7b7fe8`. Fresh-base run [`37225783702`](https://github.com/tailrocks/velnor-new/actions/runs/37225783702) tested head `071f5f236efb7248ab38773da7281331c5faa07f`; it succeeded with 20 jobs: 18 CI jobs and `Required` passed; `Publish baseline` was skipped. Its 12 Rust jobs reported 2,889 executed, 2,889 passed, zero failed, and one skipped. Selected below means executed plus skipped; Nextest does not print a separate selected count.

| Rust job | Selected | Executed | Passed | Failed | Skipped |
| --- | ---: | ---: | ---: | ---: | ---: |
| velnor-actions-cli | 229 | 229 | 229 | 0 | 0 |
| velnor-actions-rust | 193 | 193 | 193 | 0 | 0 |
| velnor-actions-contract | 197 | 197 | 197 | 0 | 0 |
| velnor-actions-tofu | 316 | 316 | 316 | 0 | 0 |
| velnor-runner-cli | 21 | 21 | 21 | 0 | 0 |
| velnor-actions-workflow-renderer | 319 | 319 | 319 | 0 | 0 |
| velnor-actions-actionlint | 69 | 69 | 69 | 0 | 0 |
| velnor-actions-orchestrator | 1,040 | 1,040 | 1,040 | 0 | 0 |
| velnor-runner-core | 12 | 12 | 12 | 0 | 0 |
| velnor-runner-github | 48 | 48 | 48 | 0 | 0 |
| velnor-actions-mise | 339 | 338 | 338 | 0 | 1 |
| velnor-runner-host | 107 | 107 | 107 | 0 | 0 |

This run verifies Velnor source and CI only. It does not verify ChainArgos consumer adoption, a published generator, a deployed worker product, or performance improvement. The current G0 PR #46 candidate is `234820e84b8c75ce2bf5adeb4fd6a890541d9ab7`, based on `70bcae7b23d7a6c47fb299338f8cc650737d712a`; it is open but not published or adopted. Its recorded local gates and two exact-head reviews passed on that base. PR #45 then changed `SUPPORTED_TARGETS` to Linux x64 and macOS ARM; PR #46 is being synchronized and its release outputs aligned to those two targets before fresh checks/reviews. No new generator asset or consumer adoption exists yet. E18 remains the distinct ChainArgos attempt-2 baseline; its Plan log used immutable generator source `47815c83b9eeadbaf84b741918fffa7ea550da89`.

Bounded local host inspection refreshed at `2026-10-04T23:30:12Z` against current main `3c3dd81e5938d72486dfc5b77b8d273237845a82` found no authorized remote Scale Set endpoint or SSH alias. Current host config has no endpoint field; there is no Velnor launchd service/process, and the only SSH alias is local OrbStack (`orb` at `127.0.0.1:32222`). Docker contexts point only to local sockets. Environment variable names were checked without reading values; no Velnor/Scale Set/remote-host name was present. No engine was contacted. This blocks current daemon inspection from this machine but does not establish the state of a remote product.

## E20 — Controlled Postgres readiness comparison (Observed)

On 4 October 2026, I ran three sequential fresh Postgres containers, then one locked Testcontainers comparison. The endpoint was local outer OrbStack on an ARM64 Mac, with `linux/amd64` emulation. This does not represent private DinD or a live Scale Set worker.

The registry index digest was `sha256:77f585114c32fbca283dc835b0596f4e52b51b4c6662d7810b2f4084f60a1873`. Its `linux/amd64` manifest was `sha256:d8703cd7fba306b9fec9268ecedfa8a966846c053036a60e3635791957eb2f66`; that manifest names config digest `sha256:c293117fcecda7344b5480222e813b9f673d7abd69b1dd95eff239b768b04f59`. The test requested that immutable platform manifest. Container inspection recorded the immutable `Config.Image`, `linux/amd64`, one CPU, 1 GiB memory, `running`, and `OOMKilled=false`. The registry metadata and test log are saved under `postgres-probe-20261004T2130Z/`.

The three direct starts each emitted the temporary-init readiness marker on stdout and the final-server marker on stderr. A combined two-marker count completed in 2.933 s, 3.371 s, and 3.075 s. `pg_isready` and TCP `SELECT 1` succeeded for all three. The goal-labeled containers were removed.

The comparison used `testcontainers` 0.27.3 and `testcontainers-modules` 0.15.0 from the consumer lockfile. Both the module's default sequential stderr-then-stdout wait and a combined `stdout_or_stderr(...).with_times(2)` wait passed with the same image and default 60-second timeout. In this one run, `.start()` took 3.947 s and 2.384 s. SQL took 81 ms and 76 ms. Cleanup took 142 ms and 144 ms. The Rust test selected and executed one test: one passed, zero failed, zero ignored, and zero cancelled. The test attempted both wait strategies. These single samples do not show a speed improvement. The successful default wait does not explain the historical CI startup timeout. No wallet selector or full consumer suite ran.

Raw evidence: `testcontainers-comparison.log` SHA-256 `0fed123c105af59b79a969e0fe518d7aa846dec4df802bbfa77fda87bbd26c7c`; `outer-docker-captures.tar.gz` SHA-256 `542f4a7ffb21186b6694382792cf9589d5e6287c8e8586c41700676da6c46a8b`; `registry-index-after.txt` SHA-256 `430ab47dc513631dac716dc5f4f3ef31bad2297c208dd3bd068de1bafd842f86`; `amd64-manifest-after.json` SHA-256 `d8703cd7fba306b9fec9268ecedfa8a966846c053036a60e3635791957eb2f66`.

## E21 — Current consumer refs and run refresh (Observed)

GitHub metadata was refreshed at `2026-10-04T22:07:26Z`. The default branch is `main` at `0a937e0c0442782bfb88c43cfdf67c1fc3f3b4f0`, which matches local `origin/main`. PR #2085 remains open and not draft. Its head is `7cbe1db11ccabb463053a93a8a08d33bda418b29`, its base is `0a937e0c0442782bfb88c43cfdf67c1fc3f3b4f0`, and its test merge is `364d8b2837a66e29333b7017388bb9ad90da469e`. No new consumer commit or generator adoption is present.

The latest pull-request event run for this PR head remains [`37178675286`](https://github.com/ChainArgos/java-monorepo/actions/runs/37178675286), attempt 2, conclusion `failure`, updated at `2026-10-04T17:03:42Z`. Its attempt-2 job API returns 45 jobs: 40 success, two failure, two cancelled, and one skipped. The failures are `legacy-grpc-server` and the aggregate `Required` job. `eth-processor-app` and `tron-processor-app` are cancelled in the attempt-2 jobs API. The API does not give the cancellation cause. This agrees with E18. Some `workflow_dispatch` runs also use the same PR head; they are not a new PR event or a consumer adoption run. The latest such runs at refresh included `37201062816` (failure) and `37201058609` (success). Do not present either as complete consumer verification.

The latest repository-wide run at refresh was [`37233531197`](https://github.com/ChainArgos/java-monorepo/actions/runs/37233531197). It is a successful pull-request run on the unrelated `platform/design-system-extraction` branch at `d29e69d0502cd926af8ff0dcb4e5418ade68dd4d`. It is not PR #2085 evidence.

Sources: [default branch API](https://api.github.com/repos/ChainArgos/java-monorepo/branches/main), [PR #2085 API](https://api.github.com/repos/ChainArgos/java-monorepo/pulls/2085), [run attempt 2 jobs API](https://api.github.com/repos/ChainArgos/java-monorepo/actions/runs/37178675286/attempts/2/jobs?per_page=100), [latest PR #2085 head runs](https://api.github.com/repos/ChainArgos/java-monorepo/actions/runs?head_sha=7cbe1db11ccabb463053a93a8a08d33bda418b29&per_page=100), [latest repository runs](https://api.github.com/repos/ChainArgos/java-monorepo/actions/runs?per_page=5).
