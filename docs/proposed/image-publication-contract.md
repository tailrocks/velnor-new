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

## Planned: jobs

| Job | Event | Permissions | Repository code |
|---|---|---|---|
| `image-<id>-<platform>` | PR, push, tag | `contents: read` | yes |
| `image-publish-<id>` | `push` to the default branch after `Required` succeeded | `packages: write`, `id-token: write`, `attestations: write`, `contents: read`; environment `images` | none |
| `image-promote-<id>` | release workflow on `<id>-v<X.Y.Z>` | `packages: write`, `attestations: read` | none |

The build job runs the prebuilt step, creates a task-owned Buildx builder and
writes an OCI layout with SBOM and `mode=max` provenance. It joins `Required`.
The publish job downloads layouts, verifies digests, pushes with a qualified
`crane` under an isolated `DOCKER_CONFIG`, and attests the digest. Promotion adds
the `<X.Y.Z>` tag to the digest already built on the default branch (hard
decision 8); a tag never rebuilds.

No private credential enters a BuildKit build: no secret mount, no `type=gha`
cache (it hands `ACTIONS_RUNTIME_TOKEN` to BuildKit), no build argument that
carries a token. A build that needs private sources must declare `prebuilt`
(`image_build_needs_private_source`, wired when private sources land).
