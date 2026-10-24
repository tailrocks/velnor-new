# ubuntu-26.04 runner image

Platform: `linux/amd64`. ARM64 is not the baseline.

The Runner image tag is `velnor-runner:ubuntu-26.04-2.338.0`.

```sh
cat > "${TMPDIR:-/tmp}/velnor-zig-musl-linker" <<'EOF'
#!/bin/sh
exec zig cc -target x86_64-linux-musl "$@"
EOF
chmod 0755 "${TMPDIR:-/tmp}/velnor-zig-musl-linker"
mise --no-config --no-env --no-hooks exec rust@1.99.1 -- \
  rustup target add x86_64-unknown-linux-musl
export VELNOR_SOURCE_REVISION="$(git rev-parse HEAD)"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER="${TMPDIR:-/tmp}/velnor-zig-musl-linker"
export RUSTFLAGS='-C link-self-contained=no'
mise --no-config --no-env --no-hooks exec rust@1.99.1 -- \
  cargo build --locked --manifest-path crates/velnor-runner/Cargo.toml \
    --package velnor-resource-probe --bin velnor-resource-probe --release \
    --target x86_64-unknown-linux-musl
bash images/resource-probe/build-runner-images.sh
```

Install Zig `0.17.0` before this build. The release workflow builds the helper
with its pinned Rust toolchain and the exact release source commit.

Set the OrbStack Linux guest memory to at least 20 GiB. The GNU tar build uses
a 14 GiB temporary filesystem at `/tmp`. The mount lets the build run executable files. The build
uses a memory limit of 20 GiB and a CPU quota of 400000.

The base is pinned to the Ubuntu 26.04 `linux/amd64` manifest digest in
`.velnor/runner.lock`. Official `actions/runner` `2.338.0` linux-x64 comes
from source commit `197ca027b90de199464dee3bb9ff198a1db60967`; its archive
SHA256 is
`af4b794c1bc41d73d40535e3fe092a39f9679cd8d965954c2aca25a05ca41d32`.
The official archive is extracted as published; Runner.Listener and its
embedded Node 24 runtime are not patched. No `/proc` spoof, no fake
`os-release`.

The image does a check of the archive SHA256 before it extracts Runner into
`/opt/runner`. The official dependency installer must leave every Runner seed
entry unchanged. The image does a check of the Runner seed with a deterministic
manifest. The Runner seed stays owned by root. The manifest stays outside the
Runner home volume.

The entrypoint does a check of a fresh Runner home volume before it reads JIT
input. It does a check of the volume owner, Work volume mount, Runner seed
manifest, and free space. It copies the full Runner seed into `/home/runner`
through a private staging directory. It does not copy `_work`. It starts only the
listener from `/home/runner` after it makes sure the copied files are correct.
The Runner seed and a 512 MiB allowance must fit within the 2 GiB Runner home volume
limit. The allowance is for Runner configuration and diagnostics.

`/usr/bin/tar.gnu` is the byte-verified GNU tar 1.35 executable. The build does a check of Ubuntu's signed source
index, source files, all 28 Ubuntu patches, and one local GNUlib declaration
patch. The build does a check of the package and executable hashes. The source
lock is `gnu-tar-source.lock.json`. The build sets
`ac_cv_func_openat2=no`, so GNU tar uses its bundled fallback. The source
build instructions do not prove atomic behavior for a concurrent directory
rename that uses `openat2`.

The image registers a local diversion from the `/usr/bin/tar` package path to
`/usr/bin/tar.ubuntu` before installing the guard, so later `dpkg` updates do
not replace the guarded frontdoor. Job commands that resolve `tar` through
`/usr/bin/tar` use the Velnor guard, which preflights archive member names and
selected symlink paths before calling the same pinned GNU executable. The guard
clears `TAR_OPTIONS` before classifying or delegating any command. Without `-P`,
it passes the original arguments to GNU tar, which applies its native absolute
and parent-traversal path rules. With `-P`, the guard preflights selected
symlink paths, targets, and ancestors, including the relative external-cache
symlink exception. Relative archive (`-f`) and member-list (`-T`) paths are
resolved from the command's starting directory; each relative `-C` is resolved
from the preceding extraction directory. Guarded `-P` extraction rejects
directory changes after member selection and unsupported selector or
transformation options rather than delegating an ambiguous request. It uses
GNU tar for the actual listing, extraction, PAX metadata, hardlinks, and
decompression.
`/usr/bin/tar.gnu` is the explicit native GNU bypass for image-owned trusted
archive operations and native behavior tests.

PAX stores paths and metadata that do not fit the old tar format. The cache
tests use the pinned Linux `actions/cache` save and restore commands for gzip.
The tests use the same action's save, list, and restore commands for zstd
without long mode.
Other tests include GNU tar zstd long mode, PAX paths, and long manifests. Native
GNU tar uses its documented path rules. Without `-P`, GNU tar rejects member
names with parent path components and strips a leading slash from absolute
names. GNU tar blocks extraction through archive and pre-existing symlinks.
With `-P`, GNU tar keeps absolute and parent paths; the guarded entrypoint
preserves the prior selected-symlink boundary before those paths reach GNU tar.
Use `-P` only with trusted archives. The guard does not snapshot an archive or
promise transactional rollback. GNU tar can leave extracted files after an
error.

`zstd` is installed so cache archives match hosted runners.
`git-lfs` is installed so `git lfs` is on `PATH`.
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

The image is not privileged and has no host Docker socket, host home directory,
Keychain, SSH agent, or controller config. UID `1000` (`runner`), group
`docker` GID `999`. In-container passwordless sudo is for job steps only.
The Runner keeps its container root read-only, receives the same 14 ordinary
capabilities Docker grants by default, and uses a pinned seccomp profile that
blocks mount and namespace changes. Job writes go to its private Home and Work
volumes, `/run` socket volume, and charged `/tmp` and `/dev/shm` tmpfs mounts.
The controller sets `DOCKER_HOST=tcp://127.0.0.1:2375` and joins this Runner
to its paired DinD container's network namespace. The DinD daemon listens only
on that namespace's loopback and the private Unix socket. No host port is
published. `/.dockerenv` remains present; Testcontainers uses the fixed
`DOCKER_HOST`, verifies HTTPS certificates normally, and receives the returned
published port on `localhost`. Container actions continue to use the private
Unix socket mounted at `/var/run/docker.sock`. The image does not override Cargo's job count. Workflows may set
`CARGO_BUILD_JOBS` or pass Cargo's `-j` option according to their own resource
budget; the runner does not infer compile parallelism from host `nproc`.

`disableUpdate=true` is a scale-set registration invariant. This image does
not set it and does not set `RUNNER_ALLOW_RUNASROOT`.

The controller must:

- Pass one JIT payload on stdin, or on inherited fd 3 if that fd is already
  open. The entrypoint exits with a non-zero status for empty stdin or fd 3.
  Do not put JIT in Docker `Config.Env`,
  `Cmd`, labels, build args, host argv, journal, or evidence. The entrypoint
  rejects a JIT value supplied in its own environment and ignores arguments.
- Leave the entrypoint as `/usr/local/bin/velnor-runner-entrypoint` (mode `0755`).
- Mount this worker's private socket so `/var/run/docker.sock` is not the
  outer engine socket. Do not mount the host socket, host home directory, Keychain,
  SSH agent, or controller config.
- Mount a fresh Runner home volume at `/home/runner`.
- Set the Runner home volume root owner to UID/GID `1000:1000`.
- Set `NoCopy` on the Runner home volume mount.
- Mount a separate Work volume at `/home/runner/_work`.
- Set `NoCopy` on the Work volume mount.
- Mount the same Runner home volume and Work volume in this worker's DinD
  container. DinD must not copy the Runner seed into that volume.
- Register the JIT runner with `workFolder: "_work"`. The official Runner
  resolves that folder under `/home/runner`. It contains work, `_temp`,
  `_actions`, and `_tool`. Both containers use the same paths.
- Start the Runner entrypoint. The entrypoint copies the Runner seed into the
  Runner home volume before it reads JIT input. The entrypoint writes raw JIT
  data to a private `/tmp` file with mode `0600`. The entrypoint removes the file before
  listener start. The entrypoint puts JIT in the
  `ACTIONS_RUNNER_INPUT_JITCONFIG` process environment when it runs
  `Runner.Listener run`. The command arguments contain only `run`.
  `CommandSettings` reads the value into its input map and removes the
  environment variable. It adds the value to `SecretMasker`. The Runner
  does not add `--jitconfig` to the command arguments. The maximum raw JIT
  size is 131040 bytes.
  The calculation uses the 131072-byte Linux limit for one exec string on a system with 4-KiB pages.
  This limit includes the environment key, `=`, and the final NUL. The official runner may
  make its own one-job Runner configuration and credential files under
  `/home/runner`. These files stay on the Runner home volume after the
  Runner container exits. Remove this owned volume after terminal cleanup.
  The JIT is not secret from its job. Same-UID processes may examine the
  listener's initial `/proc/1/environ` and accessible runner files. Controller
  API credentials never enter the runner. The Runner container runs one job. The job can read the JIT. Live tests must
  measure whether the host or another job's Runner or DinD container can read
  these values. A host or controller administrator can examine the Runner
  container and its process environment and arguments.
  `externals` is `/home/runner/externals`.

`runner-home-test.py` does a check of Runner seed files and copy rules
with a fixture. `jit-input-test.sh` does a check of the entrypoint with a
synthetic canary. These tests do not run the official listener. These tests
give no evidence that the host or another job's containers can read these
values. These tests give no evidence about which values same-job processes can
read.
