# macOS Scale Set evidence

Resolved 2026-10-03 with `gh api` from this machine. No live Scale Set session,
LaunchAgent install, image build, or ChainArgos rollout has been attempted.
Those rows stay `NOT_RUN` until a later log cites a command and its result.

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
| `actions/scaleset` | `e6daac702355cdb5b880b4fbdcf6d85dcd9e48e5` | unchanged |
| `actions/runner` | `d7bc179baf11a02110b46cfbbc4040f74ac3f60a` | unchanged |
| runner release | `v2.337.0` linux x64 SHA-256 `70920811a4f8ad4328818682bca5c6469c1c942fab52448868071d0063816613` | from the release body |
| `actions/runner-images` | `6d942e630479cd99a93dadfc766af11242bfa402` | Ubuntu 26.04 readme says generally available, image `20260927.149.1` |
| `ChainArgos/java-monorepo` main | `5f77c0b09eda3ac4a6fb8c5de0a622a0cb926c45` | unchanged |

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

GitHub CLI user `donbeave` has `repo` and `workflow` scopes and admin on both
`tailrocks/velnor-new` and `ChainArgos/java-monorepo`.

## ChainArgos coverage

Pinned `.velnor/config.toml` is schema 1, Rust `mbx`, `cargo_nextest`. The only
workflow is generated `ci.yml` (24 jobs, every `runs-on` is `ubuntu-26.04`).
No Java, Kotlin, Gradle, Bun, or frontend test job is in that file.

Ruleset `protect-main` (id 15177499) requires exactly the context `ci-required`.
That string is not a job name in `ci.yml`. Classic branch protection returned
404. Approving-review count is 0. Merge method is squash. Linear history is
required. `current_user_can_bypass` is never.

Pair verification jobs the generator already emits (20 Rust crate jobs plus
actionlint). Do not pair `plan`, `required`, or `publish-baseline`. Do not add
a Java adapter: the pinned required set does not run Java.

`ci-required` on the migration PR was a commit status waived by `donbeave`
(`CI waived for private ChainArgos repository`). HEAD at the pin had no such
status. Preserving that ruleset context is a consumer-repo concern. This
rollout must not delete the ruleset or invent a green status.

## Not yet run

No `velnor-host` binary, no scale-set create, no JIT, no LaunchAgent, no
paired workflow, no published image or macOS binary beyond the existing
`v0.1.0` generator assets.
