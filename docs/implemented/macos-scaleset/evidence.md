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
gate stays `NOT_RUN`. No ChainArgos rollout. `image-release.yml`
and `macos-binary-release.yml` were dispatched once each and returned HTTP 404
because those workflows are absent from the default branch. That attempt was
not retried. G3, the full G4 suite, G7, and G8 stay `NOT_RUN`.

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

Not a pass: `start_pair` returns only after both containers exist, so nothing was killed between DinD create and runner start. The live runner exited 1 on dummy JIT before `docker exec` (`Unexpected character` from the entrypoint). Guest path checks used a commit of that container. `/home/runner/_temp` and `/home/runner/tools` were absent. There is no public Docker cleanup that applies `delete_decision` to a live id. Scratch: `g3-matrix.txt`. G3 stays `NOT_RUN`.

## Publish attempts

Each command was run once and was not retried.

`gh workflow run image-release.yml --ref macos-scaleset --repo tailrocks/velnor-new` exited 1: `HTTP 404: workflow image-release.yml not found on the default branch (https://api.github.com/repos/tailrocks/velnor-new/actions/workflows/image-release.yml)`.

`gh workflow run macos-binary-release.yml --ref macos-scaleset --repo tailrocks/velnor-new` exited 1: `HTTP 404: workflow macos-binary-release.yml not found on the default branch (https://api.github.com/repos/tailrocks/velnor-new/actions/workflows/macos-binary-release.yml)`.

Those two dispatches were not retried. The files were absent from `main` and from this branch at that time. Later commits put generated `image-release.yml` and `macos-binary-release.yml` on `macos-scaleset` only. They are not on `main`. No image and no macOS binary were published by those 404s.

GitHub does not index a `workflow_dispatch`-only file that is not on the default branch. `main` cannot take these files: schema-1 CI diffs the whole `.github` tree, and ruleset `protect-main` (`24396608`) requires a pull request plus the `Required` check. Qualification was indexed only after commit `6b53cd8` added `push: {}`. Commit `f229bf6` did the same for the two release workflows and was pushed. GitHub then listed Image release `373713267` and macOS binary release `373713266`.

That push started both workflows at `f229bf685db2a4d296ae07159b9e250921ca17a1`:

- Image release run [37101248625](https://github.com/tailrocks/velnor-new/actions/runs/37101248625) concluded `failure`. Build runner images and Attest runner images concluded `success`. Publish runner images exited 4: `gh` had no `GH_TOKEN`.
- macOS binary release run [37101248540](https://github.com/tailrocks/velnor-new/actions/runs/37101248540) concluded `failure`. Build velnor-host exited 101: `package ID specification velnor-host did not match any packages`. The package is `velnor-runner-cli`. The binary name is `velnor-host`.

Commit `784399d` selects `-p velnor-runner-cli` and removes `push: {}`. The workflow ids remained. `gh workflow run macos-binary-release.yml --ref macos-scaleset` created run [37101412470](https://github.com/tailrocks/velnor-new/actions/runs/37101412470) at `784399d`. That run was cancelled: its publish step still had no `GH_TOKEN`, so it would have failed the same way as `37101248625`. The publish step now sets `GH_TOKEN` to `${{ github.token }}` on the publish job only. No release asset is recorded yet. ChainArgos was not updated. G7 and G8 stay `NOT_RUN`.

## Not yet run

No paired ChainArgos workflow. No published image or macOS binary beyond the existing `v0.1.0` generator assets. The first two release dispatches returned HTTP 404 and were not retried. Later runs `37101248625`, `37101248540`, and `37101412470` are in the publish section and are not a published asset. The named section 11 classes have job URLs above. Features dispatch `37097526498`, queue-pressure run `37098293064`, and crash run `37099950570` are recorded above. G4 stays `NOT_RUN` because the job log does not show the official runner cgroup compatibility check. The G3 matrix attempt is recorded and is not a pass. No promotion onto `main`.
