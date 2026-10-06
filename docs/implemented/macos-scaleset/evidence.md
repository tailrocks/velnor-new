# macOS Scale Set evidence

Resolved 2026-10-03 with `gh api` from this machine. Offline generator and
runner tests cited below have run. A per-user LaunchAgent was installed,
observed, and removed. Image builds were inspected. The product scale set was
created with the shipped client. Six ordinary scale-set echo jobs have run on
the official runner, including the four that stayed queued after an earlier
ack-without-start. Later one-class runs covered JavaScript, services,
artifacts, Buildx, an expected failure, and the classes in the second table
below, plus Compose, bind mounts, Testcontainers, submodules/LFS,
same-port workers, and cancel-with-service in the third table. The full G4
gate stays `BLOCKED_EXTERNAL`. ChainArgos now pins the PR 16 generator
commit; that rollout is not G7. The first `image-release.yml` and
`macos-binary-release.yml` dispatches returned HTTP 404 and were not
retried. Later registered runs published GitHub release assets from `19a43f5`
(image run `37102027384`, macOS run `37102029367`). Those assets are not a
GHCR push, not a new generator, and not the ChainArgos pin. G3 is `PASS`.
G7 and G8 stay `NOT_RUN`.

## Identities

| Source | SHA or value | Versus research pin |
|---|---|---|
| `tailrocks/velnor-new` main | `b9fdb1bc27b7aeeb71264cd2195328b4c5387627` | PR 17 squash; was `c57c700459bbe1549fe7eedcb7d8689585c38986` |
| tag `v0.1.0` | `c57c700459bbe1549fe7eedcb7d8689585c38986` | not moved |
| tag `generator-d40868152f7fe0106e3ede858a411f502f00810f` | `d40868152f7fe0106e3ede858a411f502f00810f` | PR 16 squash; not `v0.1.0` |
| `v0.1.0` release `target_commitish` | `95c1d6f0f1056881e42d53846dac8ffccd7aa6a5` | not the tag object |
| `v0.1.0` release `immutable` | `false` | not a provenance proof |
| `v0.1.0` Linux asset SHA-256 | `aa7e44d6579e9c586106d120ed3658fcf1c9b041027ad9f03473e8efacd3b5d5` | checksum, not notarization |
| `v0.1.0` macOS arm64 asset SHA-256 | `b6f514b71e3d1d72978c66cecf23560e7f25ad51727e9f26c88870b77d61695f` | checksum, not notarization |
| `tailrocks/velnor` main | `3f6633252963efef0d71244aadae36516a11601e` | unchanged |
| PR 1133 head | `a2a4d9f4da2c5c7a4bee8cb55b2525f301890b3a` | moved; was `27049c4bfca42d9d5a1e8cbbe584a2658ee5a77d`; draft; 0 reviews; 70 files |
| PR 1135 head | `3074bb36c2fe4f9ca0b34deb19a67acc3eb5a9c8` | unchanged; draft; 0 reviews; 5 files |
| `actions/scaleset` | `e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5` | re-read 2026-10-03; unchanged |
| `actions/runner` | `d7bc179baf11a02110b46cfbbc4040f74ac3f60a` | re-read 2026-10-03; unchanged |
| runner release | `v2.337.0` linux x64 SHA-256 `70920811a4f8ad4328818682bca5c6469c1c942fab52448868071d0063816613` | from the release body |
| `actions/runner-images` | `6d942e630479cd99a93dadfc766af11242bfa402` | Ubuntu 26.04 readme says generally available, image `20260927.149.1` |
| `ChainArgos/java-monorepo` main | `0a937e0c0442782bfb88c43cfdf67c1fc3f3b4f0` | PR 2084 squash; was `570132119c488150d8adcea2d9334fc3654567d4` |

Open PR census on `tailrocks/velnor`: only 1133 and 1135.

## Host

macOS 27.0 (build 26A428), architecture `arm64`. Docker OrbStack 29.4.0, context
`orbstack`, engine `linux/aarch64`, socket
`unix:///Users/donbeave/.orbstack/run/docker.sock`. `docker run --platform
linux/amd64 alpine:3.22 uname -m` printed `x86_64`. Emulation is available.
The x64 baseline is the container platform, not the host.

`~/Library/Application Support/Velnor` was legacy pre-`velnor-new` state. The
operator authorized deleting it. It was removed on 2026-10-03. A fresh directory
is correct for this controller.

## Commands run on this branch

`cargo test --locked --offline -p velnor-actions-contract -p velnor-actions-orchestrator -p velnor-actions-cli -p velnor-actions-workflow-renderer --all-targets impl_schema2` passed (6 tests) at `40e08e8`. That covers schema migrate preview versus `--write`, schema 1 staying hosted, `both` splitting verification only, and dispatch override.

`illegal_scale_set_label_rejected` lives in `crates/core/velnor-actions-contract/tests/impl_schema2_routing.rs` and ran in that schema-2 filter.

`cargo test --locked --offline -p velnor-runner-github --all-targets` passed at `da68f44` (25 tests): null and omitted statistics, message id 0, empty poll, partial acquire, single-flight 401, create, refresh skip, and `encodedJITConfig`.

`cargo test --locked --offline -p velnor-runner-core --test invariants` covers occupancy and cleanup proof. `cargo test --locked --offline -p velnor-runner-host -p velnor-runner-cli --all-targets` passed on 2026-10-03 (46 tests) after the journal reconcile and runner-plan changes. `around_commits_pending_before_effect_and_hides_secret` reopens the database while the effect is running and does not store the canary. `release_permitted_gates_capacity_release` calls `Capacity::release` only after proven cleanup. `before_advertise` holds pending, uncertain, and missing docker ids, and returns unjournaled owned ids for adoption rather than deletion.

`docker build --platform linux/amd64` for `images/dind` and `images/runner/ubuntu-26.04` exited 0. Inspect: host `arm64`, engine `linux/arm64`, `velnor-dind:29.8.2` `linux/amd64` id `sha256:0a778ae8c9ec3feb9ea297534d17c7f226327e9e2ba7b81dbd5a850332271e7b`, `velnor-runner:ubuntu-26.04-2.337.0` `linux/amd64` id `sha256:577f4aa8df5490bfca303ccb26aa99fa35eed7e0d889bcc1435fe2ab103a18f2`. Runner image config env is `PATH` only, cmd is null, entrypoint is the image entrypoint, and `jitconfig` occurs 0 times in `Config`. That is not a running-worker mount inspect and does not mark G3 `PASS`.

`cargo test --locked --offline -p velnor-runner-cli --all-targets` passed at `658154c` (14 tests). `velnor-host --help` exited 0 and listed every command. `status --json` and `doctor` on a missing state directory printed `waiting_for_credentials` twice, identically, and did not create the directory.

On 2026-10-02, `gh api repos/tailrocks/velnor-new/actions/runners` and the same path for `ChainArgos/java-monorepo` returned `total_count` 0. `GET .../actions/runner-scale-sets` returned HTTP 404 on both repos. That public route is not the Scale Set session API and does not mark G4 `PASS`.

On 2026-10-03, `POST /repos/tailrocks/velnor-new/actions/runners/registration-token` and the same path for `ChainArgos/java-monorepo` both returned `HTTP/2.0 201 Created`. The response body was discarded and is not in this file. A registration token is not a scale set, not a session, and not G4.

On 2026-10-02T22:49:03Z, from `crates/velnor-runner` at parent `3857679`, `cargo run --offline --quiet -p velnor-runner-host --example ensure_set` exited 0 and printed `id=1 name=ubuntu-26.04-scale-set disable_update=true labels=velnor,ubuntu-26.04-scale-set`. The binary was the registration client in this change. A repeat of that command is idempotent and prints the same line. The set is on `tailrocks/velnor-new` in runner group 1 (`Default`). Create sends `runnerGroupId` 1. Omitting it makes the service answer `No runner group found with identifier 0`. The response field is camel-case `runnerSetting` with `disableUpdate` true. Labels are exactly `velnor` and `ubuntu-26.04-scale-set`, not hosted `ubuntu-26.04`. Actions admin host is `pipelinesghubeus4.actions.githubusercontent.com`. Status at create was offline. No JIT config, no worker container, and no GitHub job URL. This does not mark G4 `PASS`. Transcript: scratch `scaleset-ensure-post-option.log` (no token).

On 2026-10-02T23:27:52Z, `cargo run --offline --quiet -p velnor-runner-host --example probe_once` from `crates/velnor-runner` printed `set_id=1 available=false` and exited 0. The message queue URL is `https` on a different host from the admin origin. The probe polled that absolute URL once, saw no `JobAvailable` id, and deleted the session. No JIT config and no GitHub job. This does not mark G4 `PASS`. Transcript: scratch `scaleset-probe-once5.log` (no token).

GitHub CLI user `donbeave` has `gist`, `read:org`, `repo`, and `workflow` scopes and admin on both `tailrocks/velnor-new` and `ChainArgos/java-monorepo`. `tailrocks/velnor-new` has no repository rulesets. Classic branch protection on `main` returned 404.

## LaunchAgent

On 2026-10-03 the debug binary `crates/velnor-runner/target/debug/velnor-host` (mtime `2026-10-03T05:00:49Z`, host `arm64`, uid 501) ran `service install` (exit 0) and `service start` (exit 0). `launchctl print gui/501/com.tailrocks.velnor.host` showed `state = running`, `forks = 0`, `execs = 1`, and arguments equal to that absolute binary, then `daemon`, then `run`. The domain was `gui/501`, not a root LaunchDaemon. A second `daemon run` exited 1 while the agent was loaded. Its stdout and stderr were empty. `status --json` was `{"state":"waiting_for_credentials"}`. `doctor` was `{"state":"waiting_for_credentials","probe":false}`. No token field. `service stop` and `service uninstall` exited 0. A following `launchctl bootout` returned `Boot-out failed: 3: No such process`. `launchctl print` then exited 113 (`Could not find service`). The plist was absent. No `velnor-host` process remained. The daemon had created `~/Library/Application Support/Velnor` containing only `daemon.lock`; that directory was removed and was absent afterward. Transcript: scratch `launchd.log`. This is G6-launchd. It is not a GitHub job and not G4.

## ChainArgos coverage

Re-read at `570132119c488150d8adcea2d9334fc3654567d4`: `.velnor/config.toml` is still schema 1, Rust `mbx`, `cargo_nextest`. The only workflow is generated `ci.yml` (24 jobs, every `runs-on` is `ubuntu-26.04`). No Java, Kotlin, Gradle, Bun, or frontend test job is in that file. That commit is the old pin. The current pin is schema 2 and is recorded below.

Ruleset `protect-main` (id 15177499) on `ChainArgos/java-monorepo` was re-read on 2026-10-03. Its required status check context is `Required` (one entry). Approving-review count is 0. Merge method is squash. Linear history is required. Review threads must be resolved. `protect-tags` (id 15581293) is also active. Do not delete either ruleset and do not invent a green `Required` or `ci-required` status.

Pair verification jobs the generator already emits (20 Rust crate jobs plus
actionlint). Do not pair `plan`, `required`, or `publish-baseline`. Do not add
a Java adapter: the pinned required set does not run Java.

An older note recorded the required context as `ci-required`. The 2026-10-03 ruleset read does not contain that string. The required context to preserve is `Required`.

## One ordinary scale-set job

On 2026-10-03T00:53:23Z, from `crates/velnor-runner`, `VELNOR_HTTPS_TRACE=1 VELNOR_LAUNCH_POLLS=8 cargo run --locked --offline -p velnor-runner-host --example launch_once` exited 0. Session statistics `totalAssignedJobs` was 0. The next broker batch was message `100000009`, kind `JobAssigned`, labels `velnor,ubuntu-26.04-scale-set`, `stats_assigned=1`. The client called `POST .../runnerscalesets/1/generatejitconfig` (HTTP 200), started the pair, then deleted that message (HTTP 204). Printed `set_id=1 started=true runner_id=5516fe1a8344beba849773ce1ee407f1db2a25e6eb67c5f6cc6f9bda0ba4b094 dind_id=7c08215c6586e4b34179c7ec229670eebc83ff5bea4f223b3de80c90af999245`. Transcript: scratch `launch-once-8.log` (no token, no JIT body).

`gh api repos/tailrocks/velnor-new/actions/runs/37081936404/jobs` showed job [111084145716](https://github.com/tailrocks/velnor-new/actions/runs/37081936404/job/111084145716) `Verify / Velnor Scale Set / Linux x64` conclusion `success`, runner `m100000009`, runner group `Default`, labels `velnor` and `ubuntu-26.04-scale-set`. Step `Qualify scale-set lane` succeeded. Runner log line `Current runner version: '2.337.0'` and `Running job: Verify / Velnor Scale Set / Linux x64`.

Host is macOS 27.0 `arm64`. Docker server is `linux` `aarch64` 29.4.0. The runner image is `velnor-runner:ubuntu-26.04-2.337.0` `linux/amd64`, so this job ran under emulation. `docker inspect` reported runner `privileged=false`, user `runner`, platform `linux`. DinD `velnor-dind:29.8.2` was `privileged=true`. Mounts were only volumes `m100000009` at `/run` and `m100000009-work` at `/home/runner/_work`. A search of runner env, cmd, labels, and entrypoint for `jitconfig` returned 0. JIT was not in that config. This is one ordinary `run` step. Later one-class runs are recorded below. They are not the rest of spec section 11.

## Drained echo queue

Later `launch_once` sessions, one at a time and only after the previous runner container exited, minted JIT for the jobs GitHub still had queued. Each scale-set job below concluded `success` on runner group `Default` with labels `velnor` and `ubuntu-26.04-scale-set`. The hosted verify job and the compare job on the same run also concluded `success`.

| Run | Scale-set job | Runner |
| --- | --- | --- |
| [37080381197](https://github.com/tailrocks/velnor-new/actions/runs/37080381197) | [111079368918](https://github.com/tailrocks/velnor-new/actions/runs/37080381197/job/111079368918) | `m100000011` |
| [37081321566](https://github.com/tailrocks/velnor-new/actions/runs/37081321566) | [111082261265](https://github.com/tailrocks/velnor-new/actions/runs/37081321566/job/111082261265) | `m100000013` |
| [37079796187](https://github.com/tailrocks/velnor-new/actions/runs/37079796187) | [111077566055](https://github.com/tailrocks/velnor-new/actions/runs/37079796187/job/111077566055) | `m100000015` |
| [37083907366](https://github.com/tailrocks/velnor-new/actions/runs/37083907366) | [111090125130](https://github.com/tailrocks/velnor-new/actions/runs/37083907366/job/111090125130) | `m100000017` |
| [37085082494](https://github.com/tailrocks/velnor-new/actions/runs/37085082494) | [111093559291](https://github.com/tailrocks/velnor-new/actions/runs/37085082494/job/111093559291) | `m100000019` |

These are the same echo step as the first job. The one-class runs below are separate workflows.

## One-class qualification jobs

On 2026-10-03, one `launch_once` session was opened before each `gh workflow run qualification.yml --ref macos-scaleset -f mode=<class>`. `mode=features` was not used. A workflow run with several parallel scale-set jobs only delivers the first `JobAssigned` to the listening session. After that session is deleted, a new session reports `assigned=0` while the sibling jobs stay `queued`. Runs `37087944845` and `37087947204` were cancelled for that reason and are not class evidence.

Each class below ran its hosted job and its scale-set job. The compare job stayed `skipped` because its `if` is `inputs.mode == 'both'`. Scale-set runners are group `Default` with labels `velnor` and `ubuntu-26.04-scale-set`. Conclusions were re-read on 2026-10-03 with `gh api repos/tailrocks/velnor-new/actions/runs/<id>/jobs`.

| Class | Run | Hosted job | Scale-set job | Runner | Conclusion |
| --- | --- | --- | --- | --- | --- |
| JavaScript (`node -e`) | [37089483013](https://github.com/tailrocks/velnor-new/actions/runs/37089483013) | [111106558048](https://github.com/tailrocks/velnor-new/actions/runs/37089483013/job/111106558048) | [111106558263](https://github.com/tailrocks/velnor-new/actions/runs/37089483013/job/111106558263) | `m100000044` | `success` |
| services | [37089523814](https://github.com/tailrocks/velnor-new/actions/runs/37089523814) | [111106680712](https://github.com/tailrocks/velnor-new/actions/runs/37089523814/job/111106680712) | [111106680574](https://github.com/tailrocks/velnor-new/actions/runs/37089523814/job/111106680574) | `m100000046` | `success` |
| artifacts | [37089574338](https://github.com/tailrocks/velnor-new/actions/runs/37089574338) | [111106828788](https://github.com/tailrocks/velnor-new/actions/runs/37089574338/job/111106828788) | [111106829084](https://github.com/tailrocks/velnor-new/actions/runs/37089574338/job/111106829084) | `m100000048` | `success` |
| Buildx | [37089620426](https://github.com/tailrocks/velnor-new/actions/runs/37089620426) | [111106968032](https://github.com/tailrocks/velnor-new/actions/runs/37089620426/job/111106968032) | [111106967890](https://github.com/tailrocks/velnor-new/actions/runs/37089620426/job/111106967890) | `m100000050` | `success` |
| expected-negative | [37089657990](https://github.com/tailrocks/velnor-new/actions/runs/37089657990) | [111107078885](https://github.com/tailrocks/velnor-new/actions/runs/37089657990/job/111107078885) | [111107079056](https://github.com/tailrocks/velnor-new/actions/runs/37089657990/job/111107079056) | `m100000052` | `failure` (required) |

The JavaScript class is `node -e 'console.log("js-action-ok")'` plus checkout. It is not a pinned third-party JavaScript action and not a composite action. Services publishes `redis:7-alpine` on port 6379 and probes `127.0.0.1:6379`, not service DNS, and has no health check. Artifacts uses `actions/upload-artifact`. Buildx runs `docker buildx version` and a scratch Dockerfile. Negative is `echo expected-negative && exit 1`. Both lanes concluded `failure`, which is the required conclusion. Listener census lines (no tokens) were `session assigned=… available=… running=… registered=… busy=… idle=…`. Transcript: scratch `drain-classes.log`. After each start the runner and DinD containers were removed, and volumes `m{messageId}` and `m{messageId}-work` were removed.

## Further one-class runs

Same dispatch rule, at parent `c654b7d`, except the cache rerun below. One `launch_once` listened before each `gh workflow run qualification.yml --ref macos-scaleset --repo tailrocks/velnor-new -f mode=<class>`. Conclusions were re-read with `gh api repos/tailrocks/velnor-new/actions/runs/<id>/jobs`. Transcript: scratch `drain-g4rest.log` and `class-g4-results.txt`.

| Class | Run | Hosted job | Scale-set job | Runner | Conclusion |
| --- | --- | --- | --- | --- | --- |
| composite local action | [37092847053](https://github.com/tailrocks/velnor-new/actions/runs/37092847053) | [111116659864](https://github.com/tailrocks/velnor-new/actions/runs/37092847053/job/111116659864) | [111116660070](https://github.com/tailrocks/velnor-new/actions/runs/37092847053/job/111116660070) | `m100000054` | `success` |
| pinned `actions/github-script` | [37092885334](https://github.com/tailrocks/velnor-new/actions/runs/37092885334) | [111116772546](https://github.com/tailrocks/velnor-new/actions/runs/37092885334/job/111116772546) | [111116772326](https://github.com/tailrocks/velnor-new/actions/runs/37092885334/job/111116772326) | `m100000056` | `success` |
| local Docker action | [37092916760](https://github.com/tailrocks/velnor-new/actions/runs/37092916760) | [111116869797](https://github.com/tailrocks/velnor-new/actions/runs/37092916760/job/111116869797) | [111116869916](https://github.com/tailrocks/velnor-new/actions/runs/37092916760/job/111116869916) | `m100000058` | `success` |
| `container:` plus redis health and service DNS | [37092958517](https://github.com/tailrocks/velnor-new/actions/runs/37092958517) | [111116993576](https://github.com/tailrocks/velnor-new/actions/runs/37092958517/job/111116993576) | [111116993287](https://github.com/tailrocks/velnor-new/actions/runs/37092958517/job/111116993287) | `m100000060` | `success` |
| outputs, env, and `PATH` | [37093009206](https://github.com/tailrocks/velnor-new/actions/runs/37093009206) | [111117215302](https://github.com/tailrocks/velnor-new/actions/runs/37093009206/job/111117215302) | [111117146977](https://github.com/tailrocks/velnor-new/actions/runs/37093009206/job/111117146977) | `m100000062` | `success` |
| secret mask | [37093041715](https://github.com/tailrocks/velnor-new/actions/runs/37093041715) | [111117246622](https://github.com/tailrocks/velnor-new/actions/runs/37093041715/job/111117246622) | [111117246737](https://github.com/tailrocks/velnor-new/actions/runs/37093041715/job/111117246737) | `m100000064` | `success` |
| cache, first attempt | [37093073005](https://github.com/tailrocks/velnor-new/actions/runs/37093073005) | [111117416539](https://github.com/tailrocks/velnor-new/actions/runs/37093073005/job/111117416539) | [111117339937](https://github.com/tailrocks/velnor-new/actions/runs/37093073005/job/111117339937) | `m100000066` | hosted `failure`; scale-set job `success` but the save did not upload |
| OIDC request URL present | [37093107558](https://github.com/tailrocks/velnor-new/actions/runs/37093107558) | [111117449907](https://github.com/tailrocks/velnor-new/actions/runs/37093107558/job/111117449907) | [111117449706](https://github.com/tailrocks/velnor-new/actions/runs/37093107558/job/111117449706) | `m100000068` | `success` |
| post after failed main | [37093141791](https://github.com/tailrocks/velnor-new/actions/runs/37093141791) | [111117559101](https://github.com/tailrocks/velnor-new/actions/runs/37093141791/job/111117559101) | [111117558962](https://github.com/tailrocks/velnor-new/actions/runs/37093141791/job/111117558962) | `m100000070` | job `failure` (required); Post step `success` |
| cancel during `sleep 180` | [37093179531](https://github.com/tailrocks/velnor-new/actions/runs/37093179531) | [111117666603](https://github.com/tailrocks/velnor-new/actions/runs/37093179531/job/111117666603) | [111117666485](https://github.com/tailrocks/velnor-new/actions/runs/37093179531/job/111117666485) | `m100000072` | `cancelled` |
| cache, after tar shim | [37093907324](https://github.com/tailrocks/velnor-new/actions/runs/37093907324) | [111119887039](https://github.com/tailrocks/velnor-new/actions/runs/37093907324/job/111119887039) | [111119801114](https://github.com/tailrocks/velnor-new/actions/runs/37093907324/job/111119801114) | `m100000074` | `success` |

The first cache save logged `Failed to save` because `/usr/bin/tar` was BusyBox and rejected `--posix` and `--files-from`. The hosted restore then exited on `fail-on-cache-miss` for key `g4-cache-37093073005`. GNU tar 1.35 cannot replace it under qemu-user: stating an explicit path returns `ENOSYS` from `openat2`. The image now installs `zstd` and points `/usr/bin/tar` at `tar-shim.sh`, which accepts those flags and runs BusyBox. `docker build --platform linux/amd64 -t velnor-runner:ubuntu-26.04-2.337.0 images/runner/ubuntu-26.04` exited 0, and its Dockerfile probes round-tripped a gzip archive and a `zstdmt` archive. The rerun above is the one that saved and restored `cache-ok`.

The mask step's echoed value in the scale-set log was `***`. The workflow source line still shows the canary, which is the command text, not the step output. The outputs scale-set log checked `G4_ENV=outputs-ok` and `g4-path-ok`. Post-fail steps were re-read from the job API: `Main fails post runs` is `failure` and `Post Main fails post runs` is `success` on both lanes. Cancel was sent with `gh run cancel` only after the scale-set job status was `in_progress`; neither lane finished the sleep as `success`.

## Section 11 classes that were still open

One scale-set job per run, except `ports`, which kept one session and set `VELNOR_MAX_JOBS=2`. `launch_once` listened before each dispatch. Head SHAs were `9d1dcfc` for Compose, bind, Testcontainers, and submodule, and `515cc73` for cancel-with-service and ports. Transcripts: scratch `drain-more.log`, `drain-cancel.log`, `drain-ports.log`.

| Class | Run | Hosted job | Scale-set job | Runner | Conclusion |
| --- | --- | --- | --- | --- | --- |
| Compose, `depends_on` healthy | [37095989453](https://github.com/tailrocks/velnor-new/actions/runs/37095989453) | [111125901031](https://github.com/tailrocks/velnor-new/actions/runs/37095989453/job/111125901031) | [111125901225](https://github.com/tailrocks/velnor-new/actions/runs/37095989453/job/111125901225) | `m100000076` | `success` |
| bind mount of the workspace file | [37096048488](https://github.com/tailrocks/velnor-new/actions/runs/37096048488) | [111126078177](https://github.com/tailrocks/velnor-new/actions/runs/37096048488/job/111126078177) | [111126078163](https://github.com/tailrocks/velnor-new/actions/runs/37096048488/job/111126078163) | `m100000078` | `success` |
| Testcontainers `11.14.0` and Ryuk | [37096079378](https://github.com/tailrocks/velnor-new/actions/runs/37096079378) | [111126173884](https://github.com/tailrocks/velnor-new/actions/runs/37096079378/job/111126173884) | [111126173802](https://github.com/tailrocks/velnor-new/actions/runs/37096079378/job/111126173802) | `m100000080` | `success` |
| exact SHA, submodule, and LFS | [37096132369](https://github.com/tailrocks/velnor-new/actions/runs/37096132369) | [111126336324](https://github.com/tailrocks/velnor-new/actions/runs/37096132369/job/111126336324) | [111126336532](https://github.com/tailrocks/velnor-new/actions/runs/37096132369/job/111126336532) | `m100000082` | `success` |
| cancel while redis is up | [37096285742](https://github.com/tailrocks/velnor-new/actions/runs/37096285742) | [111126787008](https://github.com/tailrocks/velnor-new/actions/runs/37096285742/job/111126787008) | [111126787200](https://github.com/tailrocks/velnor-new/actions/runs/37096285742/job/111126787200) | `sa8ccd085be05` | `cancelled` |
| same port 8080, two workers | [37096417428](https://github.com/tailrocks/velnor-new/actions/runs/37096417428) | [111127173396](https://github.com/tailrocks/velnor-new/actions/runs/37096417428/job/111127173396) and [111127173277](https://github.com/tailrocks/velnor-new/actions/runs/37096417428/job/111127173277) | [111127173359](https://github.com/tailrocks/velnor-new/actions/runs/37096417428/job/111127173359) `m100000086` and [111127173391](https://github.com/tailrocks/velnor-new/actions/runs/37096417428/job/111127173391) `m100000087` | both scale-set runners | `success` |

The scale-set Testcontainers log printed `testcontainers-ok` and `ryuk-seen`. The scale-set submodule log showed `git-lfs/3.7.1` and the proof step exited 0, so `HEAD` matched `GITHUB_SHA` and the submodule and LFS markers matched. Both cancel-with-service logs printed `service-up` as step output, then `gh run cancel` ran (`cancel rc=0 while=probed`). Run `37096181766` was cancelled before that probe because its SHA did not match the listener's expected commit. It is not this proof. The two scale-set port jobs printed `port-held` at `2026-10-03T04:24:30Z` and `2026-10-03T04:24:33Z` while each held port 8080. One session started both workers. `VELNOR_MAX_JOBS` unset is still 1.

This section does not mark G4 `PASS`. A later `features` dispatch and a queue-pressure run are in the next two sections. A live crash or restart during a job is still not recorded here.

## One features dispatch

`gh workflow run qualification.yml --ref macos-scaleset -f mode=features` at `88206432b7ee0d768e5e4bd33bf4038e6f556c6c`. Run [37097526498](https://github.com/tailrocks/velnor-new/actions/runs/37097526498) concluded `success`.

| Class | Hosted job | Scale-set job | Runner |
| --- | --- | --- | --- |
| Services | [111130405529](https://github.com/tailrocks/velnor-new/actions/runs/37097526498/job/111130405529) | [111130405669](https://github.com/tailrocks/velnor-new/actions/runs/37097526498/job/111130405669) | `m100000089` |
| JavaScript actions | [111130405745](https://github.com/tailrocks/velnor-new/actions/runs/37097526498/job/111130405745) | [111130405679](https://github.com/tailrocks/velnor-new/actions/runs/37097526498/job/111130405679) | `m100000091` |
| Artifacts | [111130405766](https://github.com/tailrocks/velnor-new/actions/runs/37097526498/job/111130405766) | [111130405790](https://github.com/tailrocks/velnor-new/actions/runs/37097526498/job/111130405790) | `m100000090` |
| Buildx | [111130405881](https://github.com/tailrocks/velnor-new/actions/runs/37097526498/job/111130405881) | [111130405823](https://github.com/tailrocks/velnor-new/actions/runs/37097526498/job/111130405823) | `m100000092` |

Every non-skipped job concluded `success`. This is not a queue-pressure proof: four scale-set jobs succeeded, and no snapshot showed one still queued behind two busy workers.

While those four workers were up, `docker inspect` showed each runner `privileged=false`, user `runner`, published ports empty, mounts only `volume:/run` and `volume:/home/runner/_work`. Each DinD was privileged with the same private volumes and no published ports. Needle counts for `jitconfig`, `actions_runner_input_jitconfig`, `ghp_`, and `github_pat_` were 0 on all eight containers. Host `arm64`, Docker VM `aarch64`. Scratch: `g3-inspect-features.txt`. That inspect is not the kill-at-each-stage matrix, so this observation does not mark G3.

## Queue pressure

`mode=pressure` at `ab99eaac8a0003224c3ecfea1be80fcc68af8689`. Run [37098293064](https://github.com/tailrocks/velnor-new/actions/runs/37098293064) concluded `success`. Capacity was 2 and the admission target was 3. Listener pid 52965 wrote three `started=true` lines and then exited 0.

At `2026-10-03T04:59:31Z` the scale-set jobs were:

| Job | Name | Status then | Runner |
| --- | --- | --- | --- |
| [111132611747](https://github.com/tailrocks/velnor-new/actions/runs/37098293064/job/111132611747) | Pressure A | `in_progress` | `m100000095` |
| [111132611690](https://github.com/tailrocks/velnor-new/actions/runs/37098293064/job/111132611690) | Pressure B | `in_progress` | `m100000094` |
| [111132611829](https://github.com/tailrocks/velnor-new/actions/runs/37098293064/job/111132611829) | Pressure C | `queued` | none |

C was later admitted on `m100000097` and concluded `success`. A and B also concluded `success`. Each scale-set log had two lines ending in `pressure-ok` (the step command and the step output). Hosted Pressure A/B/C also concluded `success`. The local drain script then printed `DRAIN_FAIL` because its result checker called `time.time` before importing `time`. That script failure is not a job failure. Scratch: `pressure-queue.txt`, `drain-pressure.log`.

This does not mark G4 `PASS` by itself.

## Live crash and restart

`mode=pressure` at `ab99eaac8a0003224c3ecfea1be80fcc68af8689`. Run [37099950570](https://github.com/tailrocks/velnor-new/actions/runs/37099950570) concluded `success`.

Pressure C, job [111137374561](https://github.com/tailrocks/velnor-new/actions/runs/37099950570/job/111137374561), runner `m100000099`, container `eab5d53771609ca48c07849c47fb27c2ae1c2a45d9fa9cd3172cddfadabf7777`. At `2026-10-03T05:29:30Z` the job was `in_progress` and the container was running. `kill -9` of listener pid 69253 did not remove it. A second `launch_once` at `2026-10-03T05:29:34Z` (pid 70466) left the same container running. The job later concluded `success` and the container exited on its own. The job log has one payload line `pressure-ok`. Restart deleted the recorded session id only (`DELETE .../sessions/...` 204) and did not `docker rm` the worker.

Pressure A, job [111137374564](https://github.com/tailrocks/velnor-new/actions/runs/37099950570/job/111137374564), runner `m100000102`, container `7498be682741`. At `2026-10-03T05:33:51Z` it was `in_progress`. `kill -9` of pid 70466 left that container running with the same pid `671745`. Restart pid 83571 deleted session `c01c2ce4-f655-4e9d-9420-de511c818a2f` (204) and opened a new session while the container was still running. The job concluded `success` with one payload line `pressure-ok`. Pressure B then concluded `success` on `m100000105`.

Scratch: `g4-crash.txt`, `g4-crash-kill.txt`, `launch-crash-restart.log`. This is not a G4 `PASS`. The Actions job log has no cgroup or `AssertCompatibleOS` line.

## G3 kill and canary attempt

`start_pair` was driven for `g3matrix-p1`, `p2`, `p3`, and `g3matrix-g`. Empty JIT returned `EmptyJit` and created no volume. Host `arm64`, Docker server `aarch64`, images `linux/amd64`, guest `uname -m` `x86_64` (`VirtualApple`). That is emulation.

Runners were not privileged, user `runner`, no published ports, mounts only private volumes `/run` and `/home/runner/_work`. DinD was privileged with no published TCP. Canary and token needle counts were 0 in inspect env, cmd, labels, probe argv, and a read-only `launch.db`. A same-name foreign container with a different id stayed. `delete_decision` returned `KeepForeign`. `velnor-candidate` stayed 16 and `jackin` stayed 1.

Not a pass: `start_pair` returns only after both containers exist, so nothing was killed between DinD create and runner start. The live runner exited 1 on dummy JIT before `docker exec` (`Unexpected character` from the entrypoint). Guest path checks used a commit of that container. `/home/runner/_temp` and `/home/runner/tools` were absent. Scratch: `g3-matrix.txt`.

Later, example `stage_once` called `start_pair_until` on this Mac (`HEAD` `c7c7f08`, host `arm64`, Docker `aarch64`, OrbStack `29.4.0`). Images stayed `linux/amd64`: runner `sha256:810da0d70f751db6f0d3bbf7db5db654a935937700b2e833fad09dfb728202b0`, DinD `sha256:040102cea66280f2afbc8687621ece561ae4ffdbe24222d86b48f168b7c34ffd`. Each stop returned 0. `volumes` created no container. `dind-created` left DinD `f6cbcb6602d9` created and no runner. `dind-started` left DinD `bb95eaf84183` running and no runner. `runner-created` left runner `722a1a7cbaba` created, user `runner`, `Privileged=false`, network `container:` the DinD id, no published ports, mounts only the private volumes. `runner-started` returned while runner `eeb5f9733e57` was running, before a JIT handshake. `jit` wrote the dummy bytes; the runner then exited 1 (`not a valid Base-64 string`). That is not a job. A foreign container `d377a7f7cfc3` named `g3stage-20261003133204-20241-foreign` stayed after `remove_recorded` returned `KeepForeign` for a mismatched id, then was removed only when the owned id matched. No `g3stage-` container or volume remained. The logged inspect fields in `g3-stage.txt` do not include env or labels. A follow-up `runner-created` pair (`g3meta-20261003133525`, runner `b0f1746714e8`) and a follow-up `jit` pair (`g3jitmeta-20261003133547`, runner `1a44b0cd3a8e`, still `running` at inspect) both had env `PATH` only, cmd `/usr/local/bin/velnor-runner-entrypoint`, and no `stage-stop-not-a-jit`, `jitconfig`, `ACTIONS_RUNNER_INPUT_JITCONFIG`, `ghp_`, or `github_pat_` in `docker inspect` (needle count 0). Both pairs were removed with `Delete` and their volumes were removed. Scratch: `g3-stage.txt`, `g3-meta.txt`, `g3-jit-meta.txt`.

## Guest paths before JIT

`stage_once runner-started g3guest-20261003064723` exited 0. Runner `a7eb9014f86cf2bb9f1a6fc4fb30e53a9e12230299c19f244136e18bda6f37e9` and DinD `f674958726b55511ce82fc8c539352fba5575b97859c62b9f960455c8674492e` were running. `docker exec` ran before any JIT handoff. Pid 1 was still `velnor-runner-entrypoint`. Host `uname -m` was `arm64`. Docker engine architecture was `aarch64`, id `bc9058a0-c807-412b-a088-6c1d96ddd462`, server `29.4.0`. Both images were `linux/amd64` (runner `sha256:810da0d70f751db6f0d3bbf7db5db654a935937700b2e833fad09dfb728202b0`, DinD `sha256:040102cea66280f2afbc8687621ece561ae4ffdbe24222d86b48f168b7c34ffd`). Runner and DinD `uname -m` were `x86_64`. Runner id was `uid=1000(runner) gid=1000(runner)` plus group `999(docker)`.

`/home/runner/_work` and `/run/docker.sock` were the same device and inode in both containers (device 41). `/var/run/docker.sock` was that socket. `/home/runner/_work/_temp`, `/home/runner/_work/_actions`, and `/home/runner/_work/_tool` were absent in both (`stat: cannot statx`). `/home/runner/externals` and `/home/runner/bin/Runner.Listener` exist on the runner image filesystem and are absent in DinD. `/home/runner/_work/CaseDir/A` and `/home/runner/_work/casedir/a` were different inodes. `/home/runner/_work/a path/résumé/file` contained `guest-ok` when read from DinD. Runner `docker info` id `26343876-3718-4186-b6f5-4be781fa732b` was not the host engine id. No `config.json` was present. Runner `Privileged=false`, user `runner`, no published ports, network `container:` the DinD id. DinD was privileged, no published ports, network `bridge`. Mounts were only volumes `g3guest-20261003064723` at `/run` and `g3guest-20261003064723-work` at `/home/runner/_work`. `stage_once remove` returned `Delete` for both containers and both volumes were removed. Scratch: `g3-guest.txt`.

## One live scale-set job

Qualification run [37103831130](https://github.com/tailrocks/velnor-new/actions/runs/37103831130) at `4f512e27099d2ba1e869bcbea6abd5a43824efde` concluded `success`. Job [111148411061](https://github.com/tailrocks/velnor-new/actions/runs/37103831130/job/111148411061) (`Verify / Velnor Scale Set / Linux x64`) concluded `success` on runner `m100000108` in group `Default`, started `2026-10-03T06:49:25Z`, completed `2026-10-03T06:49:35Z`. `launch_once` printed `set_id=1 started=true runner_id=7e0e126d41c56f113f5ba16b621c48f545df99b36bb252066917ae804c1fb8c9 dind_id=948d97b50f004e26bb6f7e96bbeaf3ad6369b28f81ad374083dc140affa80edb` and exited 0. That runner container image was `velnor-runner:ubuntu-26.04-2.337.0`. It started `2026-10-03T06:49:16Z` and exited 0 at `2026-10-03T06:49:36Z`. `docker exec` at `2026-10-03T06:51:24Z` failed: `container 7e0e126d41c56f113f5ba16b621c48f545df99b36bb252066917ae804c1fb8c9 is not running`. Scratch: `launch-once-exec.txt`, `g3-jit.txt`.

No canary secret was planted in that job. This observation does not mark G3. The job log still has no cgroup or `AssertCompatibleOS` line. G4's cgroup proof is `BLOCKED_EXTERNAL` in `verification.md`.

## Live exec during sleep 180

Qualification run [37106980744](https://github.com/tailrocks/velnor-new/actions/runs/37106980744) at `3e9a619ef0355cb51fc9eab71e581eb8c446d9cf` concluded `success`. Job [111157320313](https://github.com/tailrocks/velnor-new/actions/runs/37106980744/job/111157320313) (`Cancel / Velnor Scale Set`) concluded `success` on runner `m100000113`. Dispatch was `gh workflow run qualification.yml --ref macos-scaleset --repo tailrocks/velnor-new -f mode=cancel` (exit 0). `launch_once` acquired the job. No `velnor-host` daemon was running.

`docker exec` at `2026-10-03T07:39:11Z` ran while runner `b04aaf11f64597d13083530723071bad73ad548c998be4a25d74fc26cd0d1142` and DinD `24e1e091f3fe21dd568bd014205ec6d3cfde4b1920832a9234f7b88110945b6c` were both running. Guest `uname -m` was `x86_64`. Runner id was `uid=1000(runner) gid=1000(runner)` plus group `999(docker)`. Host was `arm64`. Images were `linux/amd64`. Private Docker engine `9d2b7b7e-0163-4b7e-b04e-f53a8aa378d9` (server 29.8.2, arch `x86_64`) was not host engine `bc9058a0-c807-412b-a088-6c1d96ddd462`.

Runner `Privileged=false`, user `runner`, published ports empty, network `container:` the DinD id. Mounts were only volumes `m100000113` at `/run` and `m100000113-work` at `/home/runner/_work`. No host `docker.sock`, home, SSH, or Keychain mount. DinD was privileged, bridge network, no published ports, same two volumes.

Same device 41 and inode in both containers: `_work` `128827765`, `_temp` `128828011`, `_tool` `128828010`, `/run/docker.sock` and `/var/run/docker.sock` `128827893`. `_actions` was absent in both. `externals` was runner-image device 53 inode `128748463` and absent in DinD. `RUNNER_TEMP` and `RUNNER_TOOL_CACHE` matched those paths.

The counted secret was the controller JIT payload on the guest `Runner.Listener` argv. It was not printed. Counts of that exact string were 0 in Docker inspect (env, cmd, labels, entrypoint, and the full document), container logs, host argv, `launch.db` (including wal and shm), `.velnor` TOML, and LaunchAgent plists. The cancel job does not reference `secrets.*`, and no guest process environ contained a `GITHUB_TOKEN` key at exec time.

Cleanup removed only the containers this run created and volumes `m100000110`, `m100000110-work`, `m100000113`, and `m100000113-work`. Pre-existing container count was 24 before and 24 after. This observation does not mark G3: `_actions` was never created, no Actions secrets-context value entered the job, and kill-at-each-stage, foreign-object, and registry-auth checks were not part of this run.

## Publish attempts

Each command was run once and was not retried.

`gh workflow run image-release.yml --ref macos-scaleset --repo tailrocks/velnor-new` exited 1: `HTTP 404: workflow image-release.yml not found on the default branch (https://api.github.com/repos/tailrocks/velnor-new/actions/workflows/image-release.yml)`.

`gh workflow run macos-binary-release.yml --ref macos-scaleset --repo tailrocks/velnor-new` exited 1: `HTTP 404: workflow macos-binary-release.yml not found on the default branch (https://api.github.com/repos/tailrocks/velnor-new/actions/workflows/macos-binary-release.yml)`.

Those two dispatches were not retried. The files were absent from `main` and from this branch at that time. Later commits put generated `image-release.yml` and `macos-binary-release.yml` on `macos-scaleset` only. They are not on `main`. No image and no macOS binary were published by those 404s.

GitHub does not index a `workflow_dispatch`-only file that is not on the default branch. `main` at that time could not take these files: schema-1 CI diffed the whole `.github` tree, and ruleset `protect-main` (`24396608`) requires a pull request plus the `Required` check. Qualification was indexed only after commit `6b53cd8` added `push: {}`. Commit `f229bf6` did the same for the two release workflows and was pushed. GitHub then listed Image release `373713267` and macOS binary release `373713266`.

That push started both workflows at `f229bf685db2a4d296ae07159b9e250921ca17a1`:

- Image release run [37101248625](https://github.com/tailrocks/velnor-new/actions/runs/37101248625) concluded `failure`. Build runner images and Attest runner images concluded `success`. Publish runner images exited 4: `gh` had no `GH_TOKEN`.
- macOS binary release run [37101248540](https://github.com/tailrocks/velnor-new/actions/runs/37101248540) concluded `failure`. Build velnor-host exited 101: `package ID specification velnor-host did not match any packages`. The package is `velnor-runner-cli`. The binary name is `velnor-host`.

Commit `784399d` selects `-p velnor-runner-cli` and removes `push: {}`. The workflow ids remained. `gh workflow run macos-binary-release.yml --ref macos-scaleset` created run [37101412470](https://github.com/tailrocks/velnor-new/actions/runs/37101412470) at `784399d`. That run was cancelled: its publish step still had no `GH_TOKEN`, so it would have failed the same way as `37101248625`. The publish step now sets `GH_TOKEN` to `${{ github.token }}` on the publish job only. At `ebb7767`, `workflow_dispatch` started image run [37101622880](https://github.com/tailrocks/velnor-new/actions/runs/37101622880) and macOS run [37101625217](https://github.com/tailrocks/velnor-new/actions/runs/37101625217). Image build and attest succeeded again. Publish exited 1: `failed to run git: fatal: not a git repository` because the publish job had not checked out a repository and `gh` ran inside `assets`. macOS run `37101625217` was cancelled so it would not fail the same way after the compile. The publish job now checks out the ref and passes `-R "$GITHUB_REPOSITORY"`. At `19a43f5`, `workflow_dispatch` started image run [37102027384](https://github.com/tailrocks/velnor-new/actions/runs/37102027384) and macOS run [37102029367](https://github.com/tailrocks/velnor-new/actions/runs/37102029367). Both concluded `success`. Publish created GitHub release tags, not a GHCR push (`push-to-registry=false`). Attest succeeded. Generator release `v0.1.0` was not modified. Those image and macOS releases did not update ChainArgos. G7 and G8 stay `NOT_RUN`.

Tag `runner-19a43f57566c1179febb4a1c3967bfcde4f032aa` (published `2026-10-03T06:11:32Z`):

- `velnor-runner-linux-amd64.tar` size `444593152` sha256 `b234cad0d2668376054660bca37922bc7cdf8ef34820bf18f5a59a7e5aadc04b`
- `velnor-dind-linux-amd64.tar` size `140550144` sha256 `bd351b3c24fde1a3ef0c5a402964218c82e72e2b173f34ac49b756feea80496f`
- `SHA256SUMS` size `190` sha256 `d771ec90656b166444b70c8e1c2ea9e3806963b6c337cc73c967577fd2a3ab32`

The job's local manifest lists were `docker.io/library/velnor-runner:linux-amd64` `sha256:bb6637bbd31d68479865e234848f2aa30622745395b604271c6cf3c5c85d3d4c` and `docker.io/library/velnor-dind:linux-amd64` `sha256:5c0ba8062bf9bf762de7339ab607adf9cec997c7c24043f9265be3a04223c4ed`. Those digests are not pullable registry addresses.

Tag `binary-19a43f57566c1179febb4a1c3967bfcde4f032aa` (published `2026-10-03T06:14:13Z`):

- `velnor-host` size `1586048` sha256 `c9eb774d5c3e040d54e1765e28c82f1c97e93d85ab9c53bbf4aa5eec99a452f6`
- `SHA256SUMS` size `78` sha256 `09505a51faf714868d6b0ad8edb643ff36cff869e9c46b63427dab0ca45d82ab`

Scratch: `release-assets.log`. These assets do not include the later stage-stop commit.

A new generator release cannot be cut from `macos-scaleset`. `docs/proposed/bootstrap-and-release-contract.md` §2.1 cuts a release only from a known-good default-branch commit, and ruleset `protect-tags` (`24397132`) forbids moving `v0.1.0`. Tag `v0.1.0` remains `c57c700459bbe1549fe7eedcb7d8689585c38986`. That commit has no release workflow; the tag was uploaded outside Actions. At the 2026-10-03 evidence snapshot, `origin/main` was `b9fdb1bc27b7aeeb71264cd2195328b4c5387627` and included `generator-release.yml` from PR 16. The published tag and the ChainArgos pin are in the section below. `docs/implemented/release-gates.md` BOOT-4.7 says the protected release job is not implemented. The `19a43f5` runner and host release tags are not that pin. G7 stays `NOT_RUN`.

## Composite `_actions` while the runner was up

`gh workflow run qualification.yml --ref macos-scaleset --repo tailrocks/velnor-new -f mode=composite` exited 0 at `2026-10-03T07:56:15Z` and created run [37108023561](https://github.com/tailrocks/velnor-new/actions/runs/37108023561) at `5c64bca450b3c46af16f2198b7d45cc190a8cc03`. Hosted job [111160263745](https://github.com/tailrocks/velnor-new/actions/runs/37108023561/job/111160263745) (`Composite / GitHub hosted`) concluded `success`. Scale-set job [111160263836](https://github.com/tailrocks/velnor-new/actions/runs/37108023561/job/111160263836) was `in_progress` during the exec below.

`docker exec` at `2026-10-03T07:57:22Z` ran while runner `7454c9ccbfea8e0524cbb3fde3bbff746be0b639265199c8e82b17d1455b3874` and DinD `1f6d522c2d4cf7e7ca3423ebb2c78651aef94b14d0119f78f30f092afc78b65b` were both running. Guest `uname -m` was `x86_64`. Runner id was `uid=1000(runner)`. Private Docker engine `9b86e8d1-b239-4820-b25c-f924424cbd5d` (server 29.8.2, arch `x86_64`) was not host engine `bc9058a0-c807-412b-a088-6c1d96ddd462`.

Same device 41 and inode in both containers: `_work` `128828060`, `_temp` `128828306`, `_actions` `128828317`, `_tool` `128828305`, both socket paths `128828189`. `externals` was runner-image device 53 inode `128748463` and absent in DinD. DinD listed `/home/runner/_work/_actions/actions/checkout/3d3c42e5aac5ba805825da76410c181273ba90b1`. Runner `Privileged=false`, user `runner`, published ports empty, network `container:` the DinD id. Mounts were only volumes `m100000116` (`/run`) and `m100000116-work` (`/home/runner/_work`).

The collector then removed those two containers and both volumes (`docker rm` and `docker volume rm` exited 0). Pre-existing container count stayed 24. The scale-set job was still `in_progress` with no listener, so `gh run cancel 37108023561` was sent. That job is not a success. This run does not inject `secrets.G3_CANARY`. No registry `config.json` was written or checked. This observation does not mark G3.

## Secret canary while the runner was up

`gh workflow run qualification.yml --ref macos-scaleset --repo tailrocks/velnor-new -f mode=secret` exited 0 and created run [37108914261](https://github.com/tailrocks/velnor-new/actions/runs/37108914261) at `413f20a9b2af97f692e67cf14faec2c45d9fbca0`. Scale-set job [111162832876](https://github.com/tailrocks/velnor-new/actions/runs/37108914261/job/111162832876) (`Secret / Velnor Scale Set`, runner `m100000118`, actions/runner `2.337.0`) concluded `success`. Hosted job [111162833008](https://github.com/tailrocks/velnor-new/actions/runs/37108914261/job/111162833008) concluded `success`. The step was `test -n "$G3_CANARY" && sleep 180`. Both job logs contain `G3_CANARY: ***` and zero copies of the canary bytes.

`docker exec` during that sleep saw runner `77fd8ccd908d5af9b8e932fbf7c07bde2429eaceef3cfa9920b87a3fc258ccf7` and DinD `b8370287ebc395276ae52c81dbbdef36c501d07fc739e9cf0d23310c364bef8a` running. Guest `uname -m` was `x86_64` on host `arm64` (emulated). Runner id was `uid=1000(runner)`. Private engine `af0b9521-d8c7-4254-bb14-e83a7b38beef` (server 29.8.2, arch `x86_64`) was not host engine `bc9058a0-c807-412b-a088-6c1d96ddd462`. Runner `Privileged=false`, user `runner`, published ports empty, network `container:` the DinD id. DinD was privileged on `bridge`. Mounts were only volumes `m100000118` (`/run`) and `m100000118-work` (`/home/runner/_work`).

Same device 41 and inode in both containers: `_work` `128828624`, `_temp` `128828870`, `_tool` `128828869`, both socket paths `128828753`. `_actions` was absent in both. `externals` was runner-image device 221 inode `128748463` and absent in DinD. The runner process environment contained `G3_CANARY` with a value equal to the canary file (exact count 2). DinD count was 0. The value is not in this file. Counts of those bytes were 0 in Docker inspect (env, cmd, labels, and full), container logs, host argv, `launch.db` plus wal and shm, `.velnor` TOML (2 files), LaunchAgent plists (9 files), and the launch trace. The trace records `POST .../generatejitconfig` status 200 bytes 4389 and does not include the JIT body.

Marker text `g3-registry-marker` was written to `/home/runner/.docker/config.json` in the runner and `/root/.docker/config.json` in DinD, then read back. Those paths are container-layer files, not the work volume. The collector removed both containers and both volumes (`docker volume rm` exited 0). The ids were gone. Pre-existing container count stayed 24. `/var/lib/docker/volumes` does not exist on this OrbStack host, so the host grep was skipped. This observation does not mark G3: this job did not create `_actions`, and non-ASCII paths plus kill-at-each-stage remain the earlier rows.

## Composite `_actions` after the job succeeded

`gh workflow run qualification.yml --ref macos-scaleset --repo tailrocks/velnor-new -f mode=composite` created run [37109854949](https://github.com/tailrocks/velnor-new/actions/runs/37109854949) at `d2860f9c1e6271c6e4866ba3f183308d89140092`. Scale-set job [111165489828](https://github.com/tailrocks/velnor-new/actions/runs/37109854949/job/111165489828) (`Composite / Velnor Scale Set`, runner `m100000121`) concluded `success`. Hosted job [111165489915](https://github.com/tailrocks/velnor-new/actions/runs/37109854949/job/111165489915) concluded `success`. Containers were not removed while the job was `in_progress`.

`docker exec` while runner `2c775e3df1503e162e8645f0ed6fc91a80275b927703557477d616c5f431e7fd` and DinD `12f71a1a1199ca9ebc00b726ce6760737b1d36835449eceb8658fbc663e2f907` were running. Guest `uname -m` was `x86_64` on host `arm64`. Images were `linux/amd64`. Private engine `427b9311-e71b-428a-abdd-d662a8e7ea9c` (server 29.8.2, arch `x86_64`) was not host engine `bc9058a0-c807-412b-a088-6c1d96ddd462`. Runner `Privileged=false`, user `runner`, published ports empty, network `container:` the DinD id, binds empty, labels `velnor.role=runner` and `velnor.volume=m100000121`. DinD was privileged on `bridge`, binds empty. Mounts were only volumes `m100000121` (`/run`) and `m100000121-work` (`/home/runner/_work`).

Same device 41 and inode in both containers: `_work` `128828916`, `_temp` `128829162`, `_actions` `128829173`, `_tool` `128829161`, both socket paths `128829044`. `externals` was runner-image device 53 inode `128748463` and absent in DinD. Both containers listed `/home/runner/_work/_actions/actions/checkout/3d3c42e5aac5ba805825da76410c181273ba90b1`. Marker `g3-registry-marker` was written to the runner and DinD `config.json` paths and read back. After the job conclusion was `success`, `docker rm -f` of those two ids and `docker volume rm` of those two volumes exited 0. The ids were gone. Pre-existing count stayed 24.

## Generator release and ChainArgos canary

PR 16 squash `d40868152f7fe0106e3ede858a411f502f00810f` published tag `generator-d40868152f7fe0106e3ede858a411f502f00810f`. The assets are `velnor-actions-0.1.0-x86_64-unknown-linux-gnu` and `velnor-actions-0.1.0-aarch64-apple-darwin`, plus `.sha256` files. Git tag `v0.1.0` remains `c57c700459bbe1549fe7eedcb7d8689585c38986`.

PR 17 squash `b9fdb1bc27b7aeeb71264cd2195328b4c5387627`: `daemon run` calls `launch_once`, and a second daemon fails closed. The installed LaunchAgent binary is a local release of `030363df06a99355afc9534fa42868faf7f39500`, sha256 `8a989ac6b7592b570b6c102e4f0d916b48d4e362c3d2e989be2b8aa3fe41a8df`. It is not a newly published macos-binary-release asset.

ChainArgos hosted baseline PR run [37113202410](https://github.com/ChainArgos/java-monorepo/actions/runs/37113202410) succeeded. Main push [37114238559](https://github.com/ChainArgos/java-monorepo/actions/runs/37114238559) failed only on Post Restore MBX objects (`mbx cache export exited with code 1`). That is not a scale-set failure.

ChainArgos pins generator commit `d40868152f7fe0106e3ede858a411f502f00810f`, schema 2, `execution.mode` `hosted`. PR [2084](https://github.com/ChainArgos/java-monorepo/pull/2084) squash `0a937e0c0442782bfb88c43cfdf67c1fc3f3b4f0` added dispatch-only `qualification.yml`. Required CI [37116617913](https://github.com/ChainArgos/java-monorepo/actions/runs/37116617913) succeeded. `ci.yml` stayed `ubuntu-26.04`.

N=1 default dispatch (`mode` `both`, one scale-set job) run [37117721384](https://github.com/ChainArgos/java-monorepo/actions/runs/37117721384) succeeded on head `0a937e0c0442782bfb88c43cfdf67c1fc3f3b4f0`. This is not G7.

| Lane | Job | Result |
| --- | --- | --- |
| hosted | [111187658087](https://github.com/ChainArgos/java-monorepo/actions/runs/37117721384/job/111187658087) | labels `ubuntu-26.04`; runner GitHub Actions `1000059887`; `success` |
| scale-set | [111187658197](https://github.com/ChainArgos/java-monorepo/actions/runs/37117721384/job/111187658197) | labels `velnor`, `ubuntu-26.04-scale-set`; runner `m100000001`; group `Default`; `success`; log line `qualification-scale-set` |
| compare | [111187714536](https://github.com/ChainArgos/java-monorepo/actions/runs/37117721384/job/111187714536) | labels `ubuntu-26.04`; `success` |

Listener `set_id=3` is on ChainArgos scale set `ubuntu-26.04-scale-set`. Host architecture is `arm64`. Runner and DinD images are `amd64` (emulation). The runner container was not privileged, user `runner`, env key `PATH` only. DinD was privileged. Launch row 71, docker `e38aae1eb975`, exited. DinD was left running because the launch path does not remove containers. That is not cleanup proof.

## Not yet run

Image and macOS binaries from `19a43f5` are GitHub release assets in the publish section, not a GHCR image and not the ChainArgos generator pin. The first two release dispatches returned HTTP 404 and were not retried. Runs `37101248625`, `37101248540`, and `37101412470` failed or were cancelled before those assets existed. The named section 11 classes have job URLs above. Features dispatch `37097526498`, queue-pressure run `37098293064`, and crash run `37099950570` are recorded above. G4 stays `BLOCKED_EXTERNAL` because the job log does not show the official runner cgroup compatibility check. The G3 matrix attempt is recorded and is not a pass. Guest-path exec, job `111148411061`, live exec of job `111157320313`, the composite `_actions` stat on run `37108023561`, secret run `37108914261`, and composite run `37109854949` are recorded above. G3 is PASS from those rows together. Squash merge of PR 14 is `28b83bddb1ea1a231b37f3b6619e69a768c698c9`. No workflow on that commit publishes a new `velnor-actions` tag. Tag `v0.1.0` was not moved. Image run `37110093259` and macOS binary run `37110095182` were started from `main` and are not finished here. The ChainArgos N=1 run above is not G7. G7 stays `NOT_RUN`. G8 stays `NOT_RUN`.

## Ambiguous launch journal migration proof (2026-10-05)

Version-zero journals did not record whether a failed launch had already caused a remote effect, and `cleanup_proven` only described local Docker cleanup. The migration checks `user_version` before schema mutation, validates the full required schema inside its transaction, converts legacy failed launch rows to `Uncertain`, and clears stale local cleanup proof from legacy pending/uncertain launch rows. Future versions and malformed schemas fail closed without partial migration. `Done` cleanup proof and new-version definite failures retain their existing semantics. The pinned runner protocol still has no authoritative settlement/cancellation for an ambiguous JIT request, so quarantined capacity may remain held.

The five restart/migration regressions passed on tree `9aa014a604bc7132e79be3e45627b1ee560eda3d` (Nextest run `22861709-6762-4edd-a435-982eab3d0de7`): legacy failed/pending/uncertain rows remain occupied after restart; definite new failures remain retryable; future-version rejection and malformed-schema rollback leave persisted state unchanged. Independent source approval of the migration is recorded for `7f4339ec7ebe2146ce21e785601ed6968cb9666a`; the subsequent test-only change asserted the existing `Uncertain` classification for an HTTP 500 acknowledgement.

The current base sync is published as `b07dfbd627b7bec2b3a7f86b0d896f298e376b9b`, parented by `56f0e9934288f191e966a90b742b135a66d7ee56` and `4f6def90e7b1008626db18675d1cac129b8f2ad7`. Its author is Alexey Zhokhov and committer is GitHub; the merge message contains neither required trailer. It is preserved unchanged. A local, unpushed equivalent merge `ddbf309fb55b7611f51ad94ec4616a2450c70ba8` had the same parents and tree but is not part of the published branch. This record preserves both identities without rewriting published history.
