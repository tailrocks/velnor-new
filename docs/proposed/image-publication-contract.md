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

Not yet implemented for this mode: the MBX target cache (plain `cargo` is used),
and `linux_arm64`. The shared Mise tools cache is qualified only for the Ubuntu
x86-64 hosted images (runtime identity script, composite prelude and Mise pins);
generation rejects `cargo_bin` with `linux_arm64` as
`image_prebuilt_platform_unsupported:linux_arm64` until that cache contract is
extended. Plain Dockerfile images already build on arm64.

## Planned: jobs

| Job | Event | Permissions | Repository code |
|---|---|---|---|
| `image-<id>-<platform>` | PR, push, tag | `contents: read` | yes |
| `image-publish-<id>` | `push` to the default branch after `Required` succeeded | `packages: write`, `id-token: write`, `attestations: write`, `contents: read`; environment `images` | none |
| `image-promote-<id>` | release workflow on `<id>-v<X.Y.Z>` | `packages: write`, `attestations: read` | none |

The build job creates a task-owned Buildx builder and
writes an OCI layout with SBOM and `mode=max` provenance. It joins `Required`.
The publish job downloads layouts, verifies digests, pushes with a qualified
`crane` under an isolated `DOCKER_CONFIG`, and attests the digest. Promotion adds
the `<X.Y.Z>` tag to the digest already built on the default branch (hard
decision 8); a tag never rebuilds.

No private credential enters a BuildKit build: no secret mount, no `type=gha`
cache (it hands `ACTIONS_RUNTIME_TOKEN` to BuildKit), no build argument that
carries a token. A build that needs private sources must declare `prebuilt`
(`image_build_needs_private_source`, wired when private sources land).
