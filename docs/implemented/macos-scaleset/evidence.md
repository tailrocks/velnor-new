# macOS Scale Set evidence

Resolved 2026-10-03 with `gh api` from this machine. Offline generator and
runner tests cited below have run. A per-user LaunchAgent was installed,
observed, and removed. Image builds were inspected. The product scale set was
created with the shipped client. No live JIT worker and no ChainArgos rollout.
G3, G4, G7, and G8 stay `NOT_RUN`.

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

## Not yet run

No JIT worker, no paired workflow, and no published image or macOS binary beyond the existing `v0.1.0` generator assets. Scale set id 1 exists and has no official runner job.
