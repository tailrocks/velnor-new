# ubuntu-26.04 runner image

Platform: `linux/amd64`. ARM64 is not the baseline.

```sh
docker build --platform linux/amd64 --build-arg VELNOR_SOURCE_SHA="$(git rev-parse HEAD)" -t velnor-runner:ubuntu-26.04-2.338.0 images/runner/ubuntu-26.04
```

## linux/arm64 developer build

On an Apple Silicon Mac, the same Dockerfiles build native `linux/arm64`
runner and DinD images. These images are built locally, are never published,
and do not go through image qualification. An ARM result is never recorded as
the amd64 baseline. The runner host accepts only `linux/amd64`, so
`velnor-runner` does not start these images.

`.velnor/runner.lock` pins the arm64 build. `base_image_arm64_digest` is the
`linux/arm64/v8` child of the same Ubuntu manifest list as the amd64 base.
The `*_arm64_sha256` keys hold the checksums of the arm64 downloads, at the
same versions as the amd64 downloads. The Dockerfile has no default for an
arm64 checksum, so the build fails unless each checksum is passed from the
lock. The image publication check fails when an architecture arm of either
Dockerfile differs from the URLs and checksums in the lock. A named build
context replaces the pinned amd64 base with the arm64 child. The helper programs are built for `aarch64-unknown-linux-musl`, and
they are static like the amd64 `x86_64-unknown-linux-musl` helpers.

Run from the repository root.

```sh
STAGES="$(mktemp -d)"
lock() { sed -n "s/^$1 = \"\(.*\)\"\$/\1/p" .velnor/runner.lock; }
docker run --rm --platform linux/arm64 -v "$PWD:/src:ro" -v velnor-arm64-target:/target -v "$STAGES:/out" \
  -e CARGO_TARGET_DIR=/target rust:1.99.1-trixie@sha256:8a4e90c7946b74626488b1446f712c0fe0ed1a596cbd3d871684bc9c5433ae07 \
  sh -c 'rustup target add aarch64-unknown-linux-musl \
    && cargo build --locked --release --manifest-path /src/crates/velnor-runner/Cargo.toml --target aarch64-unknown-linux-musl \
      -p velnor-resource-probe --bin velnor-resource-probe --bin velnor-storage-init --bin velnor-job-start-hook \
    && mkdir -p /out/resource-probe /out/storage-init /out/job-start-hook /out/job-started-hook \
    && cp /target/aarch64-unknown-linux-musl/release/velnor-resource-probe /out/resource-probe/resource-probe \
    && cp /target/aarch64-unknown-linux-musl/release/velnor-storage-init /out/storage-init/storage-init \
    && cp /target/aarch64-unknown-linux-musl/release/velnor-job-start-hook /out/job-start-hook/job-start-hook \
    && printf "#!/bin/sh\nset -eu\nexec /velnor/job-start-hook \"\$@\"\n" > /out/job-started-hook/velnor-job-start-hook.sh \
    && chmod 0555 /out/*/*'
base="$(lock base_image)=docker-image://$(lock base_image_name)@$(lock base_image_arm64_digest)"
docker buildx build --platform linux/arm64 --build-arg VELNOR_SOURCE_SHA="$(git rev-parse HEAD)" \
  --build-context "$base" \
  --build-arg RUNNER_ARM64_SHA256="$(lock runner_arm64_sha256)" \
  --build-arg DOCKER_ARM64_SHA256="$(lock docker_arm64_sha256)" \
  --build-arg COMPOSE_ARM64_SHA256="$(lock compose_arm64_sha256)" \
  --build-arg BUILDX_ARM64_SHA256="$(lock buildx_arm64_sha256)" \
  --build-arg NODE_ARM64_SHA256="$(lock node_arm64_sha256)" \
  --build-context resource_probe="$STAGES/resource-probe" \
  --build-context storage_init="$STAGES/storage-init" \
  --build-context job_start_hook="$STAGES/job-start-hook" \
  --build-context job_started_hook="$STAGES/job-started-hook" \
  --tag velnor-runner:linux-arm64 --load images/runner/ubuntu-26.04
docker buildx build --platform linux/arm64 --build-arg VELNOR_SOURCE_SHA="$(git rev-parse HEAD)" \
  --build-context "$base" \
  --build-arg DOCKER_ARM64_SHA256="$(lock docker_arm64_sha256)" \
  --build-context resource_probe="$STAGES/resource-probe" \
  --tag velnor-dind:linux-arm64 --load images/dind
docker image inspect --format '{{.Id}} {{.Architecture}}' velnor-runner:linux-arm64 velnor-dind:linux-arm64
```

Official [actions/runner `2.338.0` release](https://github.com/actions/runner/releases/tag/v2.338.0)
linux-x64, SHA256
`af4b794c1bc41d73d40535e3fe092a39f9679cd8d965954c2aca25a05ca41d32`.
The runner release is progressively deployed by GitHub; the image uses the
published runner binary and checksum from the official release. The runner's
private action runtime is Node 24 in this version.
Docker Engine `29.9.1` is the latest [Moby release](https://github.com/moby/moby/releases/tag/v29.9.1),
downloaded from the [official static archive](https://download.docker.com/linux/static/stable/x86_64/docker-29.9.1.tgz).
SHA256 `c158ecc55adb3afb01bbd3539ed3b997d20c26dbdc418080506cc1bf5243ff57`
was measured from that official HTTPS artifact because Docker does not publish
a checksum sidecar for it. The archive was 86,364,663 bytes, with Last-Modified
`2026-10-16 09:29:12 GMT` and ETag `b17406f2778a6fa5fb95a7be40c32549`.
[Docker Compose `5.6.0`](https://github.com/docker/compose/releases/tag/v5.6.0) SHA256
`40343e21ca777173e69cff5dbafeb37c6f81f3b0d57d9e597f036e95eb63e76a` and
[Buildx `0.38.0`](https://github.com/docker/buildx/releases/tag/v0.38.0) SHA256
`4fe4cc38adf48169132749b6ca22a990928db0118e3407584ee553723115d287`
match the official GitHub release asset digest metadata. [Node.js `26.12.0`](https://nodejs.org/en/blog/release/v26.12.0)
is the current stable line; its Linux x64 archive digest matches the official
[SHASUMS256.txt](https://nodejs.org/dist/v26.12.0/SHASUMS256.txt).
Unmodified. No `/proc` spoof, no fake `os-release`. `/usr/bin/tar` is
`tar-shim.sh` runs BusyBox tar. Unimplemented semantic GNU flags fail
closed. A newline in a member name fails closed. Without `-P`, a symlink
target outside the extract root fails closed. With `-P`, relative symlink
targets may resolve outside `-C`; an archive member that walks through an
archive symlink or a symlink already on disk still fails closed. Modes,
executable bits, directory timestamps, empty directories, and pax paths
longer than the ustar name field round-trip. `--zstd`, `--files-from`, and `-P` are implemented. `-v` does not
change archive bytes. `--strip-components` is unsupported and fails closed.
Ubuntu 26.04 GNU tar calls `openat2`, and qemu-user fails
that with `ENOSYS`, so GNU tar stays at `/usr/bin/tar.gnu` and is not the
`tar` on `PATH`. `zstd` is installed so cache archives match hosted runners.
`git-lfs` is installed before the tar divert so `git lfs` is on `PATH`.
Node.js `26.12.0` linux-x64 is on `PATH` for job steps. Its SHA256 is verified
against the official Node.js `SHASUMS256.txt`. That is not the runner's private
action runtime.

The image is not privileged and has no Docker socket, host home, Keychain,
SSH agent, or controller config. UID `1000` (`runner`), group `docker` GID `999`.
In-container passwordless sudo is for job steps only. The entrypoint also
removes `/.dockerenv` before the listener so Testcontainers reaches published
ports as `localhost`, matching a GitHub-hosted VM. Certificate verification
is unchanged. The image does not override Cargo's job count. Workflows may set
`CARGO_BUILD_JOBS` or pass Cargo's `-j` option according to their own resource
budget; the runner does not infer compile parallelism from host `nproc`.

`disableUpdate=true` is a scale-set registration invariant. This image does
not set it and does not set `RUNNER_ALLOW_RUNASROOT`.

Controller must:

- Pass one JIT payload on stdin, or on inherited fd 3 if that fd is already
  open. Empty stdin/fd exits non-zero. Do not put JIT in Docker Env, Cmd,
  labels, build args, or host argv. Arguments to the entrypoint are ignored.
- Leave the entrypoint as `/usr/local/bin/velnor-runner-entrypoint` (mode `0755`).
- Mount this worker's private socket so `/var/run/docker.sock` is not the
  outer engine socket. Do not mount the host socket, host home, Keychain,
  SSH agent, or controller config.
- Mount a named volume at `/home/runner/_work` writable by uid `1000`, and
  register the JIT runner with `workFolder: "_work"`. The official runner
  resolves that folder under `/home/runner`; it contains work, `_temp`,
  `_actions`, and `_tool`. Share the same absolute path with this worker's DinD
  container. The entrypoint stages JIT in a private, mode-`0600` `/tmp` file,
  removes it before listener start, and never stores it in the work volume.
  `externals` is `/home/runner/externals`.
