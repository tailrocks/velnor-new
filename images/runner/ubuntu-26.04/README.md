# ubuntu-26.04 runner image

Platform: `linux/amd64`. ARM64 is not the baseline.

```sh
docker build --platform linux/amd64 -t velnor-runner:ubuntu-26.04-2.337.0 images/runner/ubuntu-26.04
```

Official `actions/runner` `2.337.0` linux-x64, SHA256
`70920811a4f8ad4328818682bca5c6469c1c942fab52448868071d0063816613`.
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
Node.js `24.17.0` linux-x64 is on `PATH`
for job steps. That is not the runner's private action runtime.

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
