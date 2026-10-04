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
gate stays `NOT_RUN`. No ChainArgos rollout. The first `image-release.yml`
and `macos-binary-release.yml` dispatches returned HTTP 404 and were not
retried. Later registered runs published GitHub release assets from `19a43f5`
(image run `37102027384`, macOS run `37102029367`). Those assets are not a
GHCR push, not a new generator, and not a ChainArgos pin. G3, the full G4
suite, G7, and G8 stay `NOT_RUN`.

## Identities

| Source | SHA or value | Versus research pin |
|---|---|---|
| `tailrocks/velnor-new` main | `c57c700459bbe1549fe7eedcb7d8689585c38986` | unchanged |
| tag `v0.1.0` | lightweight tag on that same commit | unchanged |
| release `target_commitish` | `95c1d6f0f1056881e42d53846dac8ffccd7aa6a5` | not the tag object |
| release `immutable` | `false` | not a provenance proof |
| Linux asset SHA-256 | `aa7e44d6579e9c586106d120ed3658fcf1c9b041027ad9f03473e8efacd3b5d5` | checksum, not notarization |
| macOS arm64 asset SHA-256 | `b6f514b71e3d1d72978c66cecf23560e7f25ad51727e9f26c88870b77d61695f` | checksum, not notarization |
| `tailrocks/velnor` main | `3f6633252963efef0d71244aadae36516a11601e` | unchanged |
| PR 1133 head | `a2a4d9f4da2c5c7a4bee8cb55b2525f301890b3a` | moved; was `27049c4bfca42d9d5a1e8cbbe584a2658ee5a77d`; draft; 0 reviews; 70 files |
| PR 1135 head | `3074bb36c2fe4f9ca0b34deb19a67acc3eb5a9c8` | unchanged; draft; 0 reviews; 5 files |
| `actions/scaleset` | `e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5` | re-read 2026-10-03; unchanged |
| `actions/runner` | `d7bc179baf11a02110b46cfbbc4040f74ac3f60a` | re-read 2026-10-03; unchanged |
| runner release | `v2.337.0` linux x64 SHA-256 `70920811a4f8ad4328818682bca5c6469c1c942fab52448868071d0063816613` | from the release body |
| `actions/runner-images` | `6d942e630479cd99a93dadfc766af11242bfa402` | Ubuntu 26.04 readme says generally available, image `20260927.149.1` |
| `ChainArgos/java-monorepo` main | `570132119c488150d8adcea2d9334fc3654567d4` | moved; was `5f77c0b09eda3ac4a6fb8c5de0a622a0cb926c45` |

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

`illegal_scale_set_label_rejected` lives in `crates/velnor-actions-contract/tests/impl_schema2_routing.rs` and ran in that schema-2 filter.

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

Re-read at `570132119c488150d8adcea2d9334fc3654567d4`: `.velnor/config.toml` is still schema 1, Rust `mbx`, `cargo_nextest`. The only workflow is generated `ci.yml` (24 jobs, every `runs-on` is `ubuntu-26.04`). No Java, Kotlin, Gradle, Bun, or frontend test job is in that file.

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

While those four workers were up, `docker inspect` showed each runner `privileged=false`, user `runner`, published ports empty, mounts only `volume:/run` and `volume:/home/runner/_work`. Each DinD was privileged with the same private volumes and no published ports. Needle counts for `jitconfig`, `actions_runner_input_jitconfig`, `ghp_`, and `github_pat_` were 0 on all eight containers. Host `arm64`, Docker VM `aarch64`. Scratch: `g3-inspect-features.txt`. That inspect is not the kill-at-each-stage matrix, so G3 stays `NOT_RUN`.

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

No canary secret was planted in that job. G3 stays `NOT_RUN`. The job log still has no cgroup or `AssertCompatibleOS` line. G4's cgroup proof is `BLOCKED_EXTERNAL` in `verification.md`.

## Live exec during sleep 180

Qualification run [37106980744](https://github.com/tailrocks/velnor-new/actions/runs/37106980744) at `3e9a619ef0355cb51fc9eab71e581eb8c446d9cf` concluded `success`. Job [111157320313](https://github.com/tailrocks/velnor-new/actions/runs/37106980744/job/111157320313) (`Cancel / Velnor Scale Set`) concluded `success` on runner `m100000113`. Dispatch was `gh workflow run qualification.yml --ref macos-scaleset --repo tailrocks/velnor-new -f mode=cancel` (exit 0). `launch_once` acquired the job. No `velnor-host` daemon was running.

`docker exec` at `2026-10-03T07:39:11Z` ran while runner `b04aaf11f64597d13083530723071bad73ad548c998be4a25d74fc26cd0d1142` and DinD `24e1e091f3fe21dd568bd014205ec6d3cfde4b1920832a9234f7b88110945b6c` were both running. Guest `uname -m` was `x86_64`. Runner id was `uid=1000(runner) gid=1000(runner)` plus group `999(docker)`. Host was `arm64`. Images were `linux/amd64`. Private Docker engine `9d2b7b7e-0163-4b7e-b04e-f53a8aa378d9` (server 29.8.2, arch `x86_64`) was not host engine `bc9058a0-c807-412b-a088-6c1d96ddd462`.

Runner `Privileged=false`, user `runner`, published ports empty, network `container:` the DinD id. Mounts were only volumes `m100000113` at `/run` and `m100000113-work` at `/home/runner/_work`. No host `docker.sock`, home, SSH, or Keychain mount. DinD was privileged, bridge network, no published ports, same two volumes.

Same device 41 and inode in both containers: `_work` `128827765`, `_temp` `128828011`, `_tool` `128828010`, `/run/docker.sock` and `/var/run/docker.sock` `128827893`. `_actions` was absent in both. `externals` was runner-image device 53 inode `128748463` and absent in DinD. `RUNNER_TEMP` and `RUNNER_TOOL_CACHE` matched those paths.

The counted secret was the controller JIT payload on the guest `Runner.Listener` argv. It was not printed. Counts of that exact string were 0 in Docker inspect (env, cmd, labels, entrypoint, and the full document), container logs, host argv, `launch.db` (including wal and shm), `.velnor` TOML, and LaunchAgent plists. The cancel job does not reference `secrets.*`, and no guest process environ contained a `GITHUB_TOKEN` key at exec time.

Cleanup removed only the containers this run created and volumes `m100000110`, `m100000110-work`, `m100000113`, and `m100000113-work`. Pre-existing container count was 24 before and 24 after. G3 stays `NOT_RUN`: `_actions` was never created, no Actions secrets-context value entered the job, and kill-at-each-stage, foreign-object, and registry-auth checks were not part of this run.

## Publish attempts

Each command was run once and was not retried.

`gh workflow run image-release.yml --ref macos-scaleset --repo tailrocks/velnor-new` exited 1: `HTTP 404: workflow image-release.yml not found on the default branch (https://api.github.com/repos/tailrocks/velnor-new/actions/workflows/image-release.yml)`.

`gh workflow run macos-binary-release.yml --ref macos-scaleset --repo tailrocks/velnor-new` exited 1: `HTTP 404: workflow macos-binary-release.yml not found on the default branch (https://api.github.com/repos/tailrocks/velnor-new/actions/workflows/macos-binary-release.yml)`.

Those two dispatches were not retried. The files were absent from `main` and from this branch at that time. Later commits put generated `image-release.yml` and `macos-binary-release.yml` on `macos-scaleset` only. They are not on `main`. No image and no macOS binary were published by those 404s.

GitHub does not index a `workflow_dispatch`-only file that is not on the default branch. `main` cannot take these files: schema-1 CI diffs the whole `.github` tree, and ruleset `protect-main` (`24396608`) requires a pull request plus the `Required` check. Qualification was indexed only after commit `6b53cd8` added `push: {}`. Commit `f229bf6` did the same for the two release workflows and was pushed. GitHub then listed Image release `373713267` and macOS binary release `373713266`.

That push started both workflows at `f229bf685db2a4d296ae07159b9e250921ca17a1`:

- Image release run [37101248625](https://github.com/tailrocks/velnor-new/actions/runs/37101248625) concluded `failure`. Build runner images and Attest runner images concluded `success`. Publish runner images exited 4: `gh` had no `GH_TOKEN`.
- macOS binary release run [37101248540](https://github.com/tailrocks/velnor-new/actions/runs/37101248540) concluded `failure`. Build velnor-host exited 101: `package ID specification velnor-host did not match any packages`. The package is `velnor-runner-cli`. The binary name is `velnor-host`.

Commit `784399d` selects `-p velnor-runner-cli` and removes `push: {}`. The workflow ids remained. `gh workflow run macos-binary-release.yml --ref macos-scaleset` created run [37101412470](https://github.com/tailrocks/velnor-new/actions/runs/37101412470) at `784399d`. That run was cancelled: its publish step still had no `GH_TOKEN`, so it would have failed the same way as `37101248625`. The publish step now sets `GH_TOKEN` to `${{ github.token }}` on the publish job only. At `ebb7767`, `workflow_dispatch` started image run [37101622880](https://github.com/tailrocks/velnor-new/actions/runs/37101622880) and macOS run [37101625217](https://github.com/tailrocks/velnor-new/actions/runs/37101625217). Image build and attest succeeded again. Publish exited 1: `failed to run git: fatal: not a git repository` because the publish job had not checked out a repository and `gh` ran inside `assets`. macOS run `37101625217` was cancelled so it would not fail the same way after the compile. The publish job now checks out the ref and passes `-R "$GITHUB_REPOSITORY"`. At `19a43f5`, `workflow_dispatch` started image run [37102027384](https://github.com/tailrocks/velnor-new/actions/runs/37102027384) and macOS run [37102029367](https://github.com/tailrocks/velnor-new/actions/runs/37102029367). Both concluded `success`. Publish created GitHub release tags, not a GHCR push (`push-to-registry=false`). Attest succeeded. Generator release `v0.1.0` was not modified. ChainArgos was not updated. G7 and G8 stay `NOT_RUN`.

Tag `runner-19a43f57566c1179febb4a1c3967bfcde4f032aa` (published `2026-10-03T06:11:32Z`):

- `velnor-runner-linux-amd64.tar` size `444593152` sha256 `b234cad0d2668376054660bca37922bc7cdf8ef34820bf18f5a59a7e5aadc04b`
- `velnor-dind-linux-amd64.tar` size `140550144` sha256 `bd351b3c24fde1a3ef0c5a402964218c82e72e2b173f34ac49b756feea80496f`
- `SHA256SUMS` size `190` sha256 `d771ec90656b166444b70c8e1c2ea9e3806963b6c337cc73c967577fd2a3ab32`

The job's local manifest lists were `docker.io/library/velnor-runner:linux-amd64` `sha256:bb6637bbd31d68479865e234848f2aa30622745395b604271c6cf3c5c85d3d4c` and `docker.io/library/velnor-dind:linux-amd64` `sha256:5c0ba8062bf9bf762de7339ab607adf9cec997c7c24043f9265be3a04223c4ed`. Those digests are not pullable registry addresses.

Tag `binary-19a43f57566c1179febb4a1c3967bfcde4f032aa` (published `2026-10-03T06:14:13Z`):

- `velnor-host` size `1586048` sha256 `c9eb774d5c3e040d54e1765e28c82f1c97e93d85ab9c53bbf4aa5eec99a452f6`
- `SHA256SUMS` size `78` sha256 `09505a51faf714868d6b0ad8edb643ff36cff869e9c46b63427dab0ca45d82ab`

Scratch: `release-assets.log`. These assets do not include the later stage-stop commit.

A new generator release cannot be cut from `macos-scaleset`. `docs/proposed/bootstrap-and-release-contract.md` §2.1 cuts a release only from a known-good default-branch commit, and ruleset `protect-tags` (`24397132`) forbids moving `v0.1.0`. `origin/main` is still `c57c700459bbe1549fe7eedcb7d8689585c38986`. That commit has no release workflow; `v0.1.0` was uploaded outside Actions. `docs/implemented/release-gates.md` BOOT-4.7 says the protected release job is not implemented. ChainArgos downloads only `velnor-actions-0.1.0-x86_64-unknown-linux-gnu`. The runner and host release tags are not that pin. G7 stays `NOT_RUN`.

## Composite `_actions` while the runner was up

`gh workflow run qualification.yml --ref macos-scaleset --repo tailrocks/velnor-new -f mode=composite` exited 0 at `2026-10-03T07:56:15Z` and created run [37108023561](https://github.com/tailrocks/velnor-new/actions/runs/37108023561) at `5c64bca450b3c46af16f2198b7d45cc190a8cc03`. Hosted job [111160263745](https://github.com/tailrocks/velnor-new/actions/runs/37108023561/job/111160263745) (`Composite / GitHub hosted`) concluded `success`. Scale-set job [111160263836](https://github.com/tailrocks/velnor-new/actions/runs/37108023561/job/111160263836) was `in_progress` during the exec below.

`docker exec` at `2026-10-03T07:57:22Z` ran while runner `7454c9ccbfea8e0524cbb3fde3bbff746be0b639265199c8e82b17d1455b3874` and DinD `1f6d522c2d4cf7e7ca3423ebb2c78651aef94b14d0119f78f30f092afc78b65b` were both running. Guest `uname -m` was `x86_64`. Runner id was `uid=1000(runner)`. Private Docker engine `9b86e8d1-b239-4820-b25c-f924424cbd5d` (server 29.8.2, arch `x86_64`) was not host engine `bc9058a0-c807-412b-a088-6c1d96ddd462`.

Same device 41 and inode in both containers: `_work` `128828060`, `_temp` `128828306`, `_actions` `128828317`, `_tool` `128828305`, both socket paths `128828189`. `externals` was runner-image device 53 inode `128748463` and absent in DinD. DinD listed `/home/runner/_work/_actions/actions/checkout/3d3c42e5aac5ba805825da76410c181273ba90b1`. Runner `Privileged=false`, user `runner`, published ports empty, network `container:` the DinD id. Mounts were only volumes `m100000116` (`/run`) and `m100000116-work` (`/home/runner/_work`).

The collector then removed those two containers and both volumes (`docker rm` and `docker volume rm` exited 0). Pre-existing container count stayed 24. The scale-set job was still `in_progress` with no listener, so `gh run cancel 37108023561` was sent. That job is not a success. This run does not inject `secrets.G3_CANARY`. No registry `config.json` was written or checked. G3 stays `NOT_RUN`.

## Secret canary while the runner was up

`gh workflow run qualification.yml --ref macos-scaleset --repo tailrocks/velnor-new -f mode=secret` exited 0 and created run [37108914261](https://github.com/tailrocks/velnor-new/actions/runs/37108914261) at `413f20a9b2af97f692e67cf14faec2c45d9fbca0`. Scale-set job [111162832876](https://github.com/tailrocks/velnor-new/actions/runs/37108914261/job/111162832876) (`Secret / Velnor Scale Set`, runner `m100000118`, actions/runner `2.337.0`) concluded `success`. Hosted job [111162833008](https://github.com/tailrocks/velnor-new/actions/runs/37108914261/job/111162833008) concluded `success`. The step was `test -n "$G3_CANARY" && sleep 180`. Both job logs contain `G3_CANARY: ***` and zero copies of the canary bytes.

`docker exec` during that sleep saw runner `77fd8ccd908d5af9b8e932fbf7c07bde2429eaceef3cfa9920b87a3fc258ccf7` and DinD `b8370287ebc395276ae52c81dbbdef36c501d07fc739e9cf0d23310c364bef8a` running. Guest `uname -m` was `x86_64` on host `arm64` (emulated). Runner id was `uid=1000(runner)`. Private engine `af0b9521-d8c7-4254-bb14-e83a7b38beef` (server 29.8.2, arch `x86_64`) was not host engine `bc9058a0-c807-412b-a088-6c1d96ddd462`. Runner `Privileged=false`, user `runner`, published ports empty, network `container:` the DinD id. DinD was privileged on `bridge`. Mounts were only volumes `m100000118` (`/run`) and `m100000118-work` (`/home/runner/_work`).

Same device 41 and inode in both containers: `_work` `128828624`, `_temp` `128828870`, `_tool` `128828869`, both socket paths `128828753`. `_actions` was absent in both. `externals` was runner-image device 221 inode `128748463` and absent in DinD. The runner process environment contained `G3_CANARY` with a value equal to the canary file (exact count 2). DinD count was 0. The value is not in this file. Counts of those bytes were 0 in Docker inspect (env, cmd, labels, and full), container logs, host argv, `launch.db` plus wal and shm, `.velnor` TOML (2 files), LaunchAgent plists (9 files), and the launch trace. The trace records `POST .../generatejitconfig` status 200 bytes 4389 and does not include the JIT body.

Marker text `g3-registry-marker` was written to `/home/runner/.docker/config.json` in the runner and `/root/.docker/config.json` in DinD, then read back. Those paths are container-layer files, not the work volume. The collector removed both containers and both volumes (`docker volume rm` exited 0). The ids were gone. Pre-existing container count stayed 24. `/var/lib/docker/volumes` does not exist on this OrbStack host, so the host grep was skipped. G3 stays `NOT_RUN`: this job did not create `_actions`, and non-ASCII paths plus kill-at-each-stage remain the earlier rows.

## Composite `_actions` after the job succeeded

`gh workflow run qualification.yml --ref macos-scaleset --repo tailrocks/velnor-new -f mode=composite` created run [37109854949](https://github.com/tailrocks/velnor-new/actions/runs/37109854949) at `d2860f9c1e6271c6e4866ba3f183308d89140092`. Scale-set job [111165489828](https://github.com/tailrocks/velnor-new/actions/runs/37109854949/job/111165489828) (`Composite / Velnor Scale Set`, runner `m100000121`) concluded `success`. Hosted job [111165489915](https://github.com/tailrocks/velnor-new/actions/runs/37109854949/job/111165489915) concluded `success`. Containers were not removed while the job was `in_progress`.

`docker exec` while runner `2c775e3df1503e162e8645f0ed6fc91a80275b927703557477d616c5f431e7fd` and DinD `12f71a1a1199ca9ebc00b726ce6760737b1d36835449eceb8658fbc663e2f907` were running. Guest `uname -m` was `x86_64` on host `arm64`. Images were `linux/amd64`. Private engine `427b9311-e71b-428a-abdd-d662a8e7ea9c` (server 29.8.2, arch `x86_64`) was not host engine `bc9058a0-c807-412b-a088-6c1d96ddd462`. Runner `Privileged=false`, user `runner`, published ports empty, network `container:` the DinD id, binds empty, labels `velnor.role=runner` and `velnor.volume=m100000121`. DinD was privileged on `bridge`, binds empty. Mounts were only volumes `m100000121` (`/run`) and `m100000121-work` (`/home/runner/_work`).

Same device 41 and inode in both containers: `_work` `128828916`, `_temp` `128829162`, `_actions` `128829173`, `_tool` `128829161`, both socket paths `128829044`. `externals` was runner-image device 53 inode `128748463` and absent in DinD. Both containers listed `/home/runner/_work/_actions/actions/checkout/3d3c42e5aac5ba805825da76410c181273ba90b1`. Marker `g3-registry-marker` was written to the runner and DinD `config.json` paths and read back. After the job conclusion was `success`, `docker rm -f` of those two ids and `docker volume rm` of those two volumes exited 0. The ids were gone. Pre-existing count stayed 24.

## Not yet run

The paragraphs above are the 2026-10-03 morning record. Later rows supersede their identities. G7 and G8 stay `NOT_RUN`.

## Recovery inventory (2026-10-03 19:40Z)

`gh api` from this machine. This section is R0. It is not a G7 or R5 pass.

| Item | Value |
|---|---|
| `tailrocks/velnor-new` main | `5a946c33cf005777feab2bc91fa4aa8e01dd58f4` (PR 24). No commits after it. |
| Working branch | `tar-absolute` `9bf909a19f7c3fc26d0203fd0f28efbdc0eafd41` (PR 25 head) |
| Consumer | `ChainArgos/java-monorepo` PR 2085, head `620df66423cdd2403b69ced1724d17fb8afd1d6f`, branch `n1-qualification`, open, mergeable, blocked |
| Consumer pin | generator `0561601c1f71a80d16e75983124b4bd69c883bb1` (not main). Darwin asset `sha256:f3af0110d98748380943f010ae0d25eca8290c74ab21ea1ac22ef338a5c80283`. Linux asset `sha256:2e084ed7d6bc1b49228a1e033ee42ae31eceb45c7c982f14baf1721d88b96256`. |
| Tag `v0.1.0` | commit `c57c700459bbe1549fe7eedcb7d8689585c38986`. Not moved. Release target `95c1d6f0f1056881e42d53846dac8ffccd7aa6a5` differs from the tag. |
| Live runner tag | `velnor-runner:ubuntu-26.04-2.337.0` manifest list `sha256:3c7e4b73e9c600760b5af420f7c3bf8e725a8284d0d5f72d07758c7e7f474c29` (local tag, not a release asset) |
| Live DinD tag | `velnor-dind:29.8.2` `sha256:67b02176948ff029862a2a0efad252a98b1f8f3becd1ef780754b19c915ac5b7` |
| Host pid 91213 | `/Users/donbeave/Library/Application Support/Velnor/velnor-host` sha256 `ef505caf0adaa18b10998d2a11899fddf6c923ada172662b8b0380465c952a34` size 27714256. Matches no published `velnor-host` asset. Closest published size is `binary-a6d053cc` at 27680592, digest `sha256:a6b5f498b0ae92e6f69d73f01f592a6c1da3b6ee33b03265fc5322d835b09b68`. |
| `host.toml` | `max_jobs = 2`, context `orbstack`, platform `linux/amd64`, set `ubuntu-26.04-scale-set`. Credential ref not recorded here. |
| Docker | OrbStack, `DOCKER_HOST=unix:///Users/donbeave/.orbstack/run/docker.sock`. Host macOS arm64. Containers `linux/amd64`. |

Open PR disposition:

| PR | State | Disposition |
|---|---|---|
| 17 | merged `b9fdb1bc27b7aeeb71264cd2195328b4c5387627` | already satisfied in main |
| 18 | open, behind, head `f2b9ef2ab0396f2b892c260818e472685ed1f367` | historical evidence only; not current deployment |
| 19 | merged `a6d053cc43778728696dd362afd936309f2aaa23` | already satisfied in main source; running binary is a different digest |
| 20 | draft, dirty, head `f53081d700d18e5f42723d5237679c4bccf72a78` | scheduled; do not merge for this recovery |
| 22 | merged `2ca2fbd63c2650656ecb4ec7974fd8f7630e7a7f` | already satisfied |
| 23 | merged `0561601c1f71a80d16e75983124b4bd69c883bb1` | already satisfied; this is the consumer generator pin |
| 24 | merged `5a946c33cf005777feab2bc91fa4aa8e01dd58f4` | already satisfied in main source |
| 25 | open, blocked, head `9bf909a19f7c3fc26d0203fd0f28efbdc0eafd41` | relevant; path collision not fixed yet |
| 26 | open, clean, head `8480ddb5ee655dfc3f6c6c04360ad6c073980f0b` | adopt the GC pin only together with a single-bundle export; green PR checks do not prove a protected cache write |

Run `37128301624` (PR 2085, attempt 4, not terminal at this snapshot): hosted jobs were success. Scale-set rust had 5 success, 2 failure, 2 in progress, 12 queued. Failures `111269118868` (bitcoin-processor-app) and `111269118871` (amq-protocol-types) lost their DinD network namespace because a sweep compared a 12-character id to `NetworkMode`. They are not tar failures and not rabbit failures. They stay failed until a same-attempt Plan rerun executes them on `3c7e4b73…` and seeded `67b02176…`. Attempt 3 `--failed` died on missing artifact `velnor-plan-r37128301624-a3` (D17).

Run `37114238559` on consumer main failed `Post Restore MBX objects` with `No space left on device` on jobs `111178048670`, `111178048789`, `111178048807`, `111178048826`, `111178048844`. Required `111181067683` failed. R1 stays `FAIL`.

R2–R8 stay open. Do not read a partial rust success as the suite.

## Snapshot 2026-10-04

This section supersedes the 19:40Z identities. It is not a G7, G8, R1, R2, R4, or R5 pass.

| Item | Value |
|---|---|
| Live runner tag | `sha256:e1a0d1dc469e5a663fdb4a26c186604f11d307ca0f32541fd14a3c20103f634b`, built from `0cd9bd5e`. Tag not moved. |
| Side tag `-stream` | `sha256:3f8e0b2befb5ad11f2f3938da87effd85f3009a297b6e43d15a9262b884afcc5` from `31d460272`. `prod_state` is present. `sysseek` count is 0. Not the live tag. |
| PR 25 | Head `31d460272`. Run `37166989497` succeeded. Thread `4173913640` resolved after that run and the side-image check. |
| Host | pid 35645, cdhash `c7e59739db26a9a40aa5247348bd6128c3554b2c`, sha256 `788f363cc6057cb060cd67fa8737fcb5a2b1d7feae17d140335a84e37e00bcac`. `max_jobs = 2`. Not `de147432d`. |
| ChainArgos | Run `37164041817` on `baa78037`, not terminal. Last count: 25 success, 1 failure, 16 queued, 1 in progress. |
| Scale-set failure | Job `111323405519` (`lightdash-csv-delivery-app`) failed `postgres_copy_adapts_chunks_and_serializes_one_receipt` with TLS `IP address mismatch`. Hosted job `111323405480` on the same Ubuntu 26.04 image family passed that test. The runner finished the job process. Not G7. |

R1 stays `FAIL`: push run `37163556069` wrote a bundle after the earlier ENOSPC, and that write is not the accepted restore. R2 stays `NOT_RUN` on `e1a0d1dc`. R4 stays `NOT_RUN`: `generator-47815c83` was published and `baa78037` was regenerated. Tag `v0.1.0` was not moved.

## Tag move 2026-10-04 03:34Z

Run `37164041817` completed `failure` at 03:32Z: 41 success, scale-set failures `111323405519` (TLS IP mismatch) and `111323405604` (shared steps exit 100), Required failed, Publish baseline skipped. Not G7.

`velnor-runner:ubuntu-26.04-2.337.0` is now `sha256:3f8e0b2befb5ad11f2f3938da87effd85f3009a297b6e43d15a9262b884afcc5` from `31d460272`. `prod_state` was rechecked immediately before the tag. `sysseek` count was 0. Old id `sha256:e1a0d1dc469e5a663fdb4a26c186604f11d307ca0f32541fd14a3c20103f634b` remains on `-dash`. Tag `v0.1.0` was not moved. Host pid 35645 was not replaced.

Cold Qualification `37174439776` and warm `37174494720` are `workflow_dispatch` `mode=both` on `n1-qualification` `baa78037`. Both concluded `success`. Scale-set containers `9b12e2bf1345` and `fdc9a5a5a69e` used `sha256:3f8e0b2b`. Verify and compare are echoes. Class jobs were skipped. Not a G7 or R5 pass. R2 stays `NOT_RUN`: no actions-cache round trip on this image.

Qualification `37174659386` (`mode=features`) concluded `failure`. Hosted JavaScript, services, artifacts, and Buildx succeeded. Scale-set JavaScript `111354675741`, artifacts `111354675647`, and Buildx `111354675613` succeeded on `sha256:3f8e0b2b`. Services scale-set `111354675665` failed `Initialize containers`: `docker version` could not open `/var/run/docker.sock` (`no such file or directory`). DinD publishes that socket only after the RabbitMQ seed. Not a G7 pass. The live image does not yet contain the listener wait.

## Socket wait 2026-10-04 03:52Z

`79a92b8a2` waits for `/run/docker.sock` before the listener. Image `sha256:1dd3f9062e914f9b4f48db4b74d579c28358eaa7cbd6c2b2f26615a243e34547` is linux/amd64, user `runner`, entrypoint `/usr/local/bin/velnor-runner-entrypoint`, calls `wait-docker-sock`, has `prod_state`, and `sysseek` count 0. That id is the live tag `velnor-runner:ubuntu-26.04-2.337.0`. `sha256:3f8e0b2b` remains on `-stream`. `sha256:e1a0d1dc` remains on `-dash`. Tag `v0.1.0` was not moved. Host pid 35645 was not replaced.

Qualification `37175314160` (`mode=features` on `n1-qualification`) concluded `success`. Scale-set services `111356618707` (labels `velnor`, `ubuntu-26.04-scale-set`) succeeded, as did scale-set JavaScript `111356618511`, artifacts `111356618508`, and Buildx `111356618403`. Hosted twins of those four classes succeeded. During the run, container `d454127701d6` was the new image id. This is not a G7 or R5 pass: `mode=both` stays echo-only, and cache, compose, and the other class modes were not run. G8 was not published.

## Class pair 2026-10-04

Live tag stayed `sha256:1dd3f906`. Cold then warm `workflow_dispatch` on `n1-qualification`. `mbx-cache-roundtrip` was not dispatched. Not a G7 or R5 pass. G8 was not published.

Both waves succeeded for js-pin `37176049046`/`37177036322`, container `37176053837`/`37177040895`, outputs `37176057253`/`37177043168`, mask `37176060119`/`37177045639`, oidc `37176063188`/`37177048459`, bind `37176069412`/`37177053888`, ports `37176079342`/`37177062781`, and pressure `37176086944`/`37177067835`.

Negative `37176083075`/`37177065242` failed `Intentional failure` on both lanes. That failure is required. Cancel `37176092508`/`37177072575` and cancel-service `37176095323`/`37177074730` were cancelled after the sleep step was in progress.

Same failure on both lanes in both waves: compose `37176044152`/`37177031899` (`Start compose`), composite `37176046612`/`37177034203`, docker-action `37176051327`/`37177038684`, and post-fail `37176066354`/`37177050996`. Checkout succeeded. `n1-qualification` had no `qualification/` tree, so the local action, compose file, testcontainers package, and submodule marker were absent. Testcontainers `37176072422`/`37177056620` (`Install and reap`) and submodule `37176075601`/`37177059993` (prove exit 2) failed the same way. Secret `37176089845`/`37177070228` failed the canary step. Cache `37176041544`/`37177029188`: the scale-set job succeeded, but `Post Save cache` warned that `velnor-tar` rejected `--posix`, so nothing was uploaded. Hosted restore missed `g4-cache-<run id>`. Not an R2 pass. Post-fail is not a post-step proof.

`4bd923e7b` accepts `tar --posix` on create. Side tag `sha256:fe791063` is linux/amd64, user `runner`, entrypoint set, jit env 0, has `wait-docker-sock` and `prod_state`, and `sysseek` count 0. An in-image `--posix` create extracted with GNU tar. The live tag stayed `sha256:1dd3f906` while ChainArgos run `37178675286` (PR 2085, head `7cbe1db`) was in progress. Not R2. Not G7. Fixtures are `7cbe1db` on `n1-qualification`. Generated workflow YAML was not edited. G8 was not published.

## Posix live tag 2026-10-04 09:27Z

Run `37178675286` completed `cancelled`: 40 success. Scale-set eth-grpc `111366755665` and eth-processor `111366755668` were cancelled at the 30 minute job cap. Lightdash `111366755759` failed with `certificate verify failed` / `IP address mismatch` and exit 100. TLS was not weakened. Required `111406491189` failed. Publish baseline was skipped. Not G7.

Rows 710, 717, 1005, and 1008 were `uncertain` launches with no container ids. They held every slot while no runner was running. They were marked `failed`, which `live_id` will not reuse. Installed host pid 35645 is still `d5e46fc40`. Not R3.

`velnor-runner:ubuntu-26.04-2.337.0` was `sha256:fe791063` for the posix waves. `sha256:1dd3f906` remains on `-wait`. Both posix waves are terminal. Cold success: cache `37192282748` (hosted restore hit `g4-cache-37192282748` after `tar --posix`), compose `37192286602`, composite `37192289900`, js-pin `37192293194`, docker-action `37192296704`, container `37192300152`, outputs `37192303406`, mask `37192306593`, oidc `37192310150`, bind `37192317927`, testcontainers `37192321841`, submodule `37192325695`, ports `37192329466`, pressure `37192336122`. Cold negative `37192332905` and secret `37192339307` failed on purpose. Cold post-fail `37192313782` failed the main step and its post step succeeded on both lanes. Cold cancel `37192342774` and cancel-service `37192348239` never started the scale-set sleep step. Warm success: cache `37193281706` (`tar --posix` saved `g4-cache-37193281706`, hosted restore hit that key), compose `37193285680`, composite `37193289960`, js-pin `37193294524`, docker-action `37193298752`, container `37193303310`, outputs `37193307024`, mask `37193310544`, oidc `37193314155`, bind `37193320859`, testcontainers `37193324107`, submodule `37193327222`, ports `37193330217`, pressure `37193336734`. Negative `37193333612` failed `Intentional failure` on both lanes. Secret `37193339802` failed `Hold secret canary` on both lanes. Post-fail `37193317521` failed `Main fails post runs` and its post step succeeded on both lanes. Cancel `37193343224`: scale-set `Sleep until cancelled` ran from `2026-10-04T10:08:50Z` until it was cancelled at `10:09:54Z`; the hosted sleep had already succeeded. Cancel-service `37193346427` was not cancelled. Its hosted probe succeeded. The scale-set probe exited 127: redis became healthy, then `nc` was not found. Not R2: both cache round trips are real, and archive security regressions beyond that key are not proven. Not R5 or G7. G8 was not published. Live tag is now `sha256:7aaac3c48d6c12c7e55704d9d873064ac63c5df04515fa43d4ee839c0f49d11e` after `nc -z` exited 0. `sha256:fe791063` stays on `-posix`. Nc cold is terminal. Cancel `37195593468`: scale-set `m100000478` and hosted both had `Sleep until cancelled` cancelled (`10:31:52Z`–`10:32:36Z` on the scale set). Cancel-service `37195597295`: scale-set `m100000479` and hosted both cancelled `Probe service then sleep` after `Initialize containers` succeeded (scale-set probe `10:32:17Z`–`10:33:13Z`). Cache `37195602237`: scale-set `m100000482` ran `tar --posix` and saved `g4-cache-37195602237`; hosted restore hit that key and `grep -qx cache-ok` ran. Not R2, R5, or G7. G8 was not published. Warm is terminal on the same tag. Cancel `37195872352`: scale-set `m100000485` slept `10:36:35Z`–`10:37:04Z` and hosted slept `10:35:56Z`–`10:37:04Z`; both were cancelled. Cancel-service `37195876085`: scale-set `m100000486` probe `10:36:53Z`–`10:37:40Z` and the hosted probe were cancelled after container init, then `Stop containers` succeeded. Cache `37195879504`: scale-set `m100000489` ran `tar --posix` and saved `g4-cache-37195879504`; hosted restore hit that key and `grep -qx cache-ok` ran. Not R2, R5, or G7. G8 was not published. On that digest, `/usr/bin/tar` is `/usr/local/bin/velnor-tar`. The deployed binary passed `collision-a-then-b`, `collision-b-then-a`, and the rest of that probe (pass=17 fail=0), and it passed paths that contain spaces. A `-P` extract of `café.txt` failed `member list mismatch at 0` because POSIX `tar.gnu -t` escapes non-ASCII. `3835a4cd7` lists with `--quoting-style=literal`. Mounting that file over the image passed the same probe, including `unicode-member` (pass=18 fail=0). The live tag was not moved. Not an R2 pass. Class wave is not terminal and is not a cold/warm pair. Both lanes succeeded: compose `37196069739`, composite `37196073681`, js-pin `37196077389`, docker-action `37196081046`, container `37196084061`, outputs `37196087327`, mask `37196090551`, oidc `37196093684`, bind `37196103663`, testcontainers `37196109304` (Velnor `m100000514`), submodule `37196114130`, and ports `37196118927` (Velnor `m100000518` and `m100000520`). Post-fail `37196098647` failed `Main fails post runs` and `Post Main fails post runs` succeeded on hosted and on `m100000508`. Negative `37196123744` failed `Intentional failure` on hosted and on `m100000523`. Secret `37196131679` failed `Hold secret canary` on hosted and on `m100000526`. Pressure `37196127636` succeeded. Hosted A/B/C succeeded. Velnor A `s23e4f5b88357` slept `10:55:56Z`–`10:58:27Z`, Velnor C `m100000525` slept `10:59:53Z`–`11:02:24Z`, and Velnor B `m100000530` slept `11:01:42Z`–`11:04:14Z`. The class wave on `sha256:7aaac3c4` is terminal and is not a cold/warm pair. Live tag is now `sha256:57d05f30897d6ba8f68aaaf1e3cc6e504c967cd2792e7fd40d0d3762ac231134`. Before the move, that image's `/usr/bin/tar` passed a `-P` extract of `café.txt`, `nc -z`, and the tar probe (pass=18 fail=0, including both collision orders and `unicode-member`). `-posix` stays `sha256:fe791063` and `-wait` stays `sha256:1dd3f906`. Cold on the new digest is terminal and is not the warm pair. Cancel `37198586022`: scale-set `m100000534` slept `11:26:19Z`–`11:26:51Z` and hosted `1000060206` slept `11:23:42Z`–`11:26:42Z`; both were cancelled. Cancel-service `37198590058`: hosted `1000060207` and scale-set `m100000535` cancelled `Probe service then sleep` after `Initialize containers` succeeded (scale-set probe `11:27:27Z`–`11:28:35Z`), then `Stop containers` succeeded. Cache `37198594869`: scale-set `m100000538` ran `tar --posix` and saved `g4-cache-37198594869`; hosted restore hit that key, `Cache restored successfully` ran, and `grep -qx cache-ok` ran. Empty, partial, corrupt, and disk-full cache cases are not proven. Warm on the same digest is terminal and is not the class pair. Cancel `37199312025`: scale-set `m100000542` slept `11:38:40Z`–`11:39:25Z` and hosted `1000060209` slept `11:36:31Z`–`11:39:25Z`; both were cancelled. Cancel-service `37199315892`: hosted and scale-set `m100000543` cancelled `Probe service then sleep` after `nc -z` printed `Connection succeeded` and `service-up` (scale-set probe `11:39:12Z`–`11:40:02Z`), then `Stop containers` succeeded. Cache `37199320139`: scale-set `m100000546` ran `tar --posix` and saved `g4-cache-37199320139`; hosted restore hit that key, `Cache restored successfully` ran, and `grep -qx cache-ok` ran. Class wave on this digest is terminal and is not a pair. Both lanes succeeded: compose `37199691629` (`m100000548`), composite `37199695990` (`m100000549`), js-pin `37199700356` (`m100000552`), docker-action `37199705022` (`m100000554`), container `37199708592` (`m100000556`), outputs `37199712654` (`m100000557`), mask `37199716686` (`m100000559`), oidc `37199720516` (`m100000561`), bind `37199728122` (`m100000566`), testcontainers `37199732602` (`m100000569`), submodule `37199737050` (`m100000570`), and ports `37199741237` (A `m100000575`, B `m100000574`). Post-fail `37199724313` failed `Main fails post runs` and `Post Main fails post runs` succeeded on hosted and on `m100000564`. Negative `37199745420` failed `Intentional failure` on hosted and on `m100000579`. Secret `37199752980` failed `Hold secret canary` on hosted and on `m100000586`. Pressure `37199749063` succeeded. Hosted A/B/C succeeded. Velnor B `m100000582` slept `11:55:54Z`–`11:58:25Z`, Velnor C `m100000583` slept `11:56:42Z`–`11:59:12Z`, and Velnor A `s3354ac3ba701` slept `11:59:56Z`–`12:02:26Z`. On this digest `/usr/bin/tar` rejected an empty `--files-from` (exit 255, `no paths`), a 700-byte cut of a 3584-byte archive (`short read`), corrupt bytes (`short header`), and a zero-length archive. Extract of a 3147264-byte archive onto a 1MiB tmpfs failed with `No space left on device`. Second class wave is in flight: compose `37201003553`, composite `37201007603`, js-pin `37201011534`, docker-action `37201015425`, container `37201019403`, outputs `37201023650`, mask `37201027693`, oidc `37201032582`, post-fail `37201036544`, bind `37201040641`, testcontainers `37201044096`, submodule `37201047607`, ports `37201051464`, negative `37201054960`, pressure `37201058609`, secret `37201062816`. Not R2, R5, G7, or G8.
