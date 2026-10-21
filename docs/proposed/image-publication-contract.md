# Container image publication contract (`[[images]]`)

**Status:** Proposed. Source: the ChainArgos split design, section 4.5. The
schema and the Dockerfile analysis are implemented in
`velnor-actions-contract` (`config::images`, `config::dockerfile`); the sections
marked **Planned** are delivered by later increments of the same lane.

Decision records: hard decision 8 in [`../hard-decisions.md`](../hard-decisions.md)
(adopted per the split brief; owner may revisit).

## Configuration

```toml
[[images]]
id = "eth-processor-app"
repository = "ghcr.io/chainargos/eth-processor-app"
dockerfile = "backend-rust/eth-processor-app/Dockerfile"   # or { linux_amd64 = "...", linux_arm64 = "..." }
context = "."
platforms = ["linux_amd64", "linux_arm64"]
version_file = "backend-rust/eth-processor-app/Cargo.toml"

[images.prebuilt]                    # optional
kind = "cargo_bin"                   # closed union; gradle_task arrives with the Gradle stack
package = "eth-processor-app"
bin = "eth-processor-app"
stage = "dist/eth-processor-app"
```

| Key | Rule |
|---|---|
| `id` | Workflow-task id grammar, at most 40 bytes, unique. Names `image-<id>-<amd64\|arm64>` and `image-publish-<id>`. |
| `repository` | `ghcr.io/<owner>/<name>[/<name>...]`, lowercase, no tag or digest, unique. The owner must equal the lowercase repository owner; the generated publish job checks this at run time. |
| `dockerfile` | One path, or a table keyed by platform whose keys equal `platforms` exactly. |
| `context` | Repo-relative directory; `.` is the root. |
| `platforms` | Non-empty, sorted, unique: `linux_amd64` (`ubuntu-26.04`), `linux_arm64` (`ubuntu-26.04-arm`). Native runners only; no QEMU. |
| `timeout_minutes` | Literal `timeout-minutes` of each build job, `1..=360`, default 30. |
| `version_file` | Manifest whose version names the release tag that promotes the image. |
| `prebuilt` | Artifacts Velnor builds outside BuildKit. `cargo_bin` requires `package`, `bin` and a `stage` path inside a non-root `context`. |

Unknown keys and unknown `prebuilt.kind` values fail deserialization. Paths are
lexically confined: no absolute, empty, `.` or `..` segments, no expressions.

## Dockerfile rules

`analyze_dockerfile` is a static parse (continuations, heredocs and global `ARG`
defaults are understood). It rejects:

- `image_base_unpinned`: any external `FROM`, `COPY --from` or `RUN --mount=from`
  image without `@sha256:<64 hex>`. Earlier stage names, stage indexes and
  `scratch` are exempt.
- `image_base_unresolved`: a `FROM` that names an `ARG` with no default.
- `image_syntax_unpinned`: a `# syntax=` frontend that is not digest-pinned.
- `image_secret_mount`: `RUN --mount=type=secret` or `type=ssh`.
- `image_add_remote_source`: `ADD` of a URL or git source.
- `image_context_escape`: a `COPY`/`ADD` source containing `..`.
- `image_escape_directive`: a changed `# escape=` character.

It also reports the context-relative `COPY`/`ADD` sources, which drive affected
selection. `RUN` may use the network (for example `apt`); it never receives a
secret.

## Build jobs (implemented)

`image-<id>-<amd64|arm64>` runs on the native hosted runner of its platform
with `contents: read` only, in schema 1 and in every schema 2 execution mode
(images are hosted-only, like build tasks; a Scale Set override is rejected).
The orchestrator resolves each Dockerfile (tracked, regular, no symlink
component, at most 1 MiB), applies the Dockerfile rules above, and binds the
file's SHA-256 into the job; the job re-checks symlinks and the digest before
building. Steps: checkout (no persisted credential), source guard, native
Docker host check, task-owned Buildx builder from the digest-pinned
`moby/buildkit` image (removed by an `always()` step), `docker buildx build
--sbom=true --provenance=mode=max --output type=oci`, then upload of
`image.tar`, `image.tar.sha256` and `metadata.json`. Every build job joins
`Required`, so pull requests validate the image build.

## Prebuilt `cargo_bin` (implemented for `linux_amd64`)

With `[images.prebuilt] kind = "cargo_bin"` the build job depends on `plan`,
gets the shared Mise setup and tools cache, and compiles before BuildKit:
pinned `rust` prepare, restored and fetched Cargo sources (same steps as a
crate job), then `cargo build --release --locked --offline --manifest-path
<pkg manifest> --package <package> --bin <bin>` under `mise --no-config
--no-env --no-hooks exec`, with `CARGO_TARGET_DIR` outside the workspace so the
build context never carries `target/`. The binary is copied to `stage`, which
must not exist beforehand. Generation fails when the package or binary is not
discovered, the binary needs `required-features`, or the Dockerfile never
`COPY`s the stage path (`image_prebuilt_stage_not_copied`). The compile runs in
the unprivileged job; BuildKit only sees the staged file, so cargo-chef-in-Docker
and any credential for private sources stay out of the image build.

When the package's lock closure contains private sources
([private sources contract](private-sources-contract.md)), the job stages the
Velnor helper and runs the same mint, fetch and revoke block as a Rust crate
job, immediately before the Cargo source fetch: the installation token is
revoked before the compile, the Dockerfile `RUN` steps or any other repository
step runs. A closure without private sources adds no credential step.

Plain Dockerfile images already build on arm64; `cargo_bin` with `linux_arm64`
and the cache wiring are deferred, with the exact blockers recorded under
[Deferred work](#deferred-work-and-its-blockers).

## Publication (implemented, `publish.yml`)

| Job | Event | Permissions | Repository code |
|---|---|---|---|
| `image-<id>-<platform>` (in `ci.yml`) | PR, push | `contents: read` | yes |
| `image-publish-<id>` | `push` to the default branch after `Required` succeeded | `contents: read`, `actions: read`, `packages: write`, `id-token: write`, `attestations: write`; environment `images` | none |
| `image-promote-<id>` | `publish.yml` on `<id>-v<X.Y.Z>`, images with a `version_file` | `contents: read`, `packages: write`, `attestations: read`; environment `images` | none |

The privileged jobs live in `publish.yml`, not `ci.yml`, because the `ci.yml`
token hygiene gate forbids any registry token in a step and its permission
type has no `packages` or `attestations` scope. In the publication IR they are
the roles `ImagePublish` and `ImagePromote` (`publish_spec.rs`): each has an
exact permission matrix, may not check out code, and transitively needs the
branch authority job (publish) or the tag authority job (promote).

`image-publish-<id>` needs the `publish-branch-authority` job, which verifies a
default-branch push in the generation-time repository and polls the `Required`
check run at the commit until it concludes (success only). It then:

1. checks that the declared registry owner equals the lowercase repository
   owner;
2. finds the successful `ci.yml` run of the same commit and push event and
   downloads each platform's `velnor-image-<id>-<arch>` artifact (build
   artifacts are kept one day, so a late re-run of the publish job fails
   closed);
3. verifies each artifact: exactly `image.tar`, `image.tar.sha256`,
   `metadata.json`; the checksum sidecar; a tar of only relative regular
   files and directories; a layout with one top-level index whose digest
   equals the build's `containerimage.digest` and its own blob bytes;
4. merges the per-platform BuildKit indexes (image manifests and attestation
   manifests, descriptors verbatim) into one multi-platform index layout and
   extracts each platform's SPDX SBOM from its BuildKit attestation;
5. logs in with `crane` into an isolated `DOCKER_CONFIG`, pushes the layout to
   `<repository>:sha-<commit>` (an existing tag with another digest fails),
   verifies the registry digest equals the assembled one, and tags the default
   branch name when it is a valid tag;
6. attests build provenance for the index digest (`attest-build-provenance`)
   and an SBOM per platform image manifest (`actions/attest`), both pushed to
   the registry;
7. runs `gh attestation verify` for the index provenance and every SBOM
   (signer workflow `publish.yml`, source digest the pushed commit).

`crane` is the catalog tool `PinnedTool::Crane`, installed through Mise's aqua
backend (the aqua registry verifies the release `checksums.txt`) and recorded
in `.velnor/version-policy.toml` and the freshness inventory like every other
pinned tool. `actions/attest` joins the action allowlist pinned by full SHA.

The workflow has no registry credential outside these jobs; PRs run only the
build jobs, which hold `contents: read`.

## Promotion (implemented, `publish.yml`)

An image with a `version_file` (a `Cargo.toml`, `gradle.properties` or
`package.json`) is releasable: its id joins the declared artifact set of the
tag authority job (`publish-authority`), which already verifies that the tag
is annotated, reachable from the default branch, has a green `Required` check
at the commit, and that the manifest version at the commit equals the tag
version. Image ids, bundle ids and released Rust package names share the
`<id>-v<X.Y.Z>` tag namespace and may not collide.

`image-promote-<id>` needs the tag authority job and runs only when the tag
names its image. It never builds, downloads a layout or checks out code:

1. resolves the digest behind `<repository>:sha-<tagged commit>`, which the
   default-branch publish job pushed;
2. runs `gh attestation verify` on that digest: the provenance must be signed
   by this repository's `publish.yml`, for the tagged commit
   (`--source-digest`) on the default branch (`--source-ref`), so a digest
   that was retagged or pushed by anything else is refused;
3. adds the tag `<X.Y.Z>` to the same digest with `crane tag`; an existing
   release tag must already name that digest, so re-running is idempotent and
   a moved tag fails.

Mixed workflows (images with a version file plus bundles or Rust releases)
condition the authority job on the pushed ref type, as bundles already do.

## Credentials

No private credential enters a BuildKit build: no secret mount, no `type=gha`
cache (it hands `ACTIONS_RUNTIME_TOKEN` to BuildKit), no build argument that
carries a token. A build that needs private sources must declare `prebuilt`
(`image_build_needs_private_source`, wired when private sources land).

## Deferred work and its blockers

### `buildkit-<id>` family and the MBX target cache for `cargo_bin`

Neither is wired. The image jobs build with plain `cargo` over the shared
Cargo sources family and a task-owned builder with no remote cache. Why a
`buildkit-<id>` family does not fit the merged contract yet:

1. The runtime-keyed family set is closed (`CacheFamily::{Sources,
   TofuProviders}`, `cache_family.rs`), and each family is threaded through
   typed surfaces: step ids and roles (`StepId::TofuProviders`,
   `StepRole::TofuProvidersRestore` and `Save`), the step-protocol validators,
   the report layer enum and the qualification cache-lineage layer. The only
   per-root locator the Plan job's key step emits is the Tofu root ordinal. A
   BuildKit family needs a member in each of those plus per image and platform
   locator outputs, which is a cache-contract change; `cache-contract.md` is at
   its 400-line limit and must be split before it can describe one.
2. [`cache-decisions.md`](cache-decisions.md) requires every hot family to
   have a measured size and a refused-and-reported save above its ceiling.
   Decisions 2 and 4 (dependency seed, per-family pruning)
   are not implemented, and `cache-measurements.md` holds no BuildKit figure. A
   `type=local,mode=max` export is unbounded without them. `type=gha` stays
   rejected because it hands `ACTIONS_RUNTIME_TOKEN` to BuildKit.
3. The MBX transport on main is the `mbx cache export` and `import` bundle of
   decision 3 (`github-cache-mode: target` is removed). Wiring `cargo_bin`
   jobs to it needs the dependency-seed family of decision 2, so no image job
   saves a bundle yet.

Unblocked by: decisions 2 to 4 landing with measured ceilings. The image
jobs then add one family (`buildkit-<id>`, schema 1, a local export directory
under `$RUNNER_TEMP/velnor/buildkit/<id>/<platform>`, trusted writes only) and
restore the MBX bundle in the prebuilt compile.

### `linux_arm64` for `cargo_bin`

Generation rejects it as `image_prebuilt_platform_unsupported:linux_arm64`.
The shared Mise tools cache and its prelude are qualified for Ubuntu x86-64
only. Exactly:

- `ReleaseTarget` has three members (Linux x86-64, macOS arm64, macOS x86-64);
  `for_runner_label("ubuntu-26.04-arm")` is `None`, so `target_for_runner`
  is `None` and finalization fails with `tools_cache_unsupported_target`.
- `MiseSetup::for_target` has binary digests for Linux x64 and the two macOS
  targets only, and no `aarch64-unknown-linux-gnu` pin.
- The runtime identity gate accepts only target `x86_64-unknown-linux-gnu`
  and fixes `RUNNER_ARCH=X64` (`lane_identity` and the shared script's
  `expected_arch` and `expected_target`); the identity prelude composites exist
  for the three x86-64 Ubuntu lanes (`TOOLS_CACHE_PRELUDE_USES`).
- `RUST_TARGET_TRIPLE` and the toolchain name derived from it are x86-64.
- No hosted evidence exists: the qualification rows of the
  [hosted cache qualification contract](hosted-cache-qualification-contract.md)
  are incomplete, and an arm64 identity (`ImageOS`, `RUNNER_ARCH=ARM64`) needs a
  source-bound hosted run on `ubuntu-26.04-arm` produced by generated workflow
  changes, not by a hand-written probe.

Not blocking: the Mise Linux arm64 digest is published in the upstream
`SHASUMS256.txt` the source-build bootstrap already uses, and Mr. Boxington
ships a Linux aarch64 asset (the bootstrap catalog pins it). Generalizing the
four code points above and running the hosted qualification unblocks it.
