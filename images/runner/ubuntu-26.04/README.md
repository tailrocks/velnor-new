# ubuntu-26.04 runner image

Platform: `linux/amd64`. ARM64 is not the baseline.

```sh
docker build --platform linux/amd64 -t velnor-runner:ubuntu-26.04-2.338.0 images/runner/ubuntu-26.04
```

The base is pinned to the Ubuntu 26.04 `linux/amd64` manifest digest in
`.velnor/runner.lock`. Official `actions/runner` `2.338.0` linux-x64 comes
from source commit `197ca027b90de199464dee3bb9ff198a1db60967`; its archive
SHA256 is
`af4b794c1bc41d73d40535e3fe092a39f9679cd8d965954c2aca25a05ca41d32`.
The official archive is extracted as published; Runner.Listener and its
embedded Node 24 runtime are not patched. No `/proc` spoof, no fake
`os-release`. `/usr/bin/tar` is
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
Node.js `26.12.0` linux-x64 is on `PATH` for job steps. That is not the
runner's private action runtime; actions use the embedded Node 24 supplied
with the unmodified official Runner archive. Docker Engine `29.9.1`, Buildx
`0.38.0`, and Compose `5.6.0` are pinned with their source records and
checksums in `.velnor/runner.lock`.

The public stable release selected here is Runner `2.338.0`. On 2026-10-18,
the three recorded repository-scoped downloads still selected `2.337.0`.
That is progressive-distribution evidence, not evidence that `2.338.0` is
rejected. Scale-set registration and a live job with this release are still
required before qualification.

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
  open. Empty stdin/fd exits non-zero. Do not put JIT in Docker `Config.Env`,
  `Cmd`, labels, build args, host argv, journal, or evidence. The entrypoint
  rejects a JIT value supplied in its own environment and ignores arguments.
- Leave the entrypoint as `/usr/local/bin/velnor-runner-entrypoint` (mode `0755`).
- Mount this worker's private socket so `/var/run/docker.sock` is not the
  outer engine socket. Do not mount the host socket, host home, Keychain,
  SSH agent, or controller config.
- Mount a named volume at `/home/runner/_work` writable by uid `1000`, and
  register the JIT runner with `workFolder: "_work"`. The official runner
  resolves that folder under `/home/runner`; it contains work, `_temp`,
  `_actions`, and `_tool`. Share the same absolute path with this worker's DinD
  container. The entrypoint stages the raw JIT in a private, mode-`0600`
  `/tmp` file, removes it before listener start, and passes it only as the
  short-lived `ACTIONS_RUNNER_INPUT_JITCONFIG` environment value on the
  `Runner.Listener run` exec. The listener's argument vector contains only
  `run`. The maximum raw JIT size is 131040 bytes.
  The calculation uses the 131072-byte Linux limit for one exec string on a system with 4-KiB pages.
  This limit includes the environment key, `=`, and the final NUL. The official runner may
  make its own one-job runner configuration and credential files under
  `/home/runner`; those files are removed with this job's runner container.
  The JIT is not secret from its job. Same-UID processes may examine the
  listener's initial `/proc/1/environ` and accessible runner files. Controller
  API credentials never enter the runner. Treat the runner container as a
  one-job boundary; it does not hide the JIT from that job. Host and
  sibling-container process isolation still require live verification.
  `externals` is `/home/runner/externals`.

`jit-input-test.sh` does a check of the entrypoint input channel with a synthetic canary and test programs.
It does not run the official listener or prove
host, sibling-container, or same-job isolation.
