# Testcontainers localhost TLS fixture

This fixture checks the Node Testcontainers SDK's mapped-port behavior with
normal HTTPS certificate verification. It copies a small HTTPS server, its leaf
certificate, and its private key into a service container from the SDK archive
API. It creates no host bind mount, fixed host port, or purpose-built service
image.

The Rust fixture writes `ca.pem`, `server.pem`, and `server-key.pem` into an
existing empty directory whose mode is `0700`. The key file must have mode
`0600`. The Node client reads the CA only. Testcontainers copies the leaf and
key to `/tmp/velnor-tls-server.pem` and
`/tmp/velnor-tls-server-key.pem`, preserving modes `0644` and `0600`; it copies
the shared server to `/tmp/velnor-tls-server.mjs` with mode `0644`. The pinned
job image runs the server as its default user, listens on container port 8443,
and writes `velnor-tls-listening` when ready.

The client requires `container.getHost()` to be exactly `localhost` and uses
`container.getMappedPort(8443)`. Its positive requests use the URL
`https://localhost:<mapped-port>` with ordinary hostname resolution, the Rust
CA, and a dedicated non-global HTTPS agent. That agent has no proxy settings.
The client verifies `/healthz` and the exact public random nonce from `/nonce`.
It then requires certificate-verification failures for an untrusted CA, a
wrong TLS DNS name, and `127.0.0.1`. It does not replace the certificate
identity check, disable TLS verification, or set a Docker host override.

Before SDK or Docker work, the Node command requires `DOCKER_HOST` to be
exactly `tcp://localhost:2375` and requires both `TESTCONTAINERS_HOST_OVERRIDE`
and `DOCKER_CONTEXT` to be absent, including when either is set to an empty
string. The local HTTPS self-test does not use or require these Docker settings.

The SDK path requires an explicitly prepared runner that shares the DinD
daemon's network namespace. Being on the same Docker bridge is insufficient
for a mapped service port to be reachable as `localhost`. The current
qualification job does not establish this namespace, so it cannot yet be
claimed as paired SDK proof. Once such a runner is separately provided, install
only the checked-in Node dependencies, create an empty private output
directory, and run the Rust certificate command before the Node script:

```sh
set -e
npm ci --prefix qualification/testcontainers
scratch="$(mktemp -d)"
trap 'rm -rf -- "$scratch"' EXIT
chmod 0700 "$scratch"
mkdir "$scratch/certs"
chmod 0700 "$scratch/certs"
/Users/donbeave/.local/bin/rust-build-guard \
  -- mise exec -- cargo run --locked \
  --manifest-path fixtures/qualification/testcontainers-rust/Cargo.toml \
  -- certs "$scratch/certs"
node qualification/testcontainers/tls.mjs "$scratch/certs"
```

The Rust command is a local preparation step. The SDK qualification only
counts as paired proof when it runs inside the digest-pinned `velnor-job`
container in the same DinD network namespace that owns the mapped port. A run
from the host shell can check the HTTPS fixture but cannot establish that
namespace contract.

The pinned Testcontainers SDK bounds its wait-strategy phase through
`withStartupTimeout` and each HTTPS request through a five-second timer. Its
`stop({ timeout: 0 })` requests immediate container stop and removal using the
SDK's default auto-remove setting. The SDK does not provide an overall
deadline for Docker API calls, and a failed `start()` may not return a
container handle for direct cleanup. With SDK auto-cleanup enabled, the reaper
tracks the run's containers and handles cleanup when the Node process exits.
The invoking job must impose a finite process/job deadline; the fixture does
not claim a hard deadline for a stalled Docker API or stop/remove call.

The Rust SDK path has separate cleanup limits. Its startup timeout bounds the
readiness phase, not image pull, container creation, file copy, or earlier
Docker API calls. After startup returns an owned handle, the fixture explicitly
awaits `container.rm()` under a ten-second timeout. If that timeout cancels
removal, dropping the handle may trigger another SDK cleanup attempt without
an overall deadline. This fixture disables Testcontainers' default features
and does not rely on a Rust reaper to clean up failed or timed-out operations.
The invoking job must enforce a finite process/job deadline for the Rust path
as well.

## Future paired SDK qualification

This procedure describes a future paired run; no current workflow establishes
the required namespace or runs both SDK commands. A local HTTPS self-test is
not evidence of the paired Docker path.

Use a separately approved Linux runner in the exact network namespace shared
with its DinD daemon. The SDKs start the existing service image
`ghcr.io/tailrocks/velnor-job@sha256:b36946574402eb0c79fb6d7a9f7a77195d2ef943c92bc6b0fbc466ff3ea408f9`.
Build the standalone Rust fixture from its checked-in `Cargo.toml` and
`Cargo.lock` on that Linux runner through the approved guard, Mise, and MBX
toolchain, and set `RUST_TLS_FIXTURE_BIN` to that Linux executable. Do not use a
macOS-built binary. Generate the private certificate bundle on the same Linux
runner as shown above, and install only the checked-in Node dependencies.

Before running either SDK, require `DOCKER_HOST=tcp://localhost:2375` and
require `TESTCONTAINERS_HOST_OVERRIDE` and `DOCKER_CONTEXT` to be absent. Then
run both clients sequentially under one finite outer deadline:

```sh
set -eu
: "${RUST_TLS_FIXTURE_BIN:?set the verified Linux fixture executable}"
: "${certs:?set the private certificate directory}"
test "$DOCKER_HOST" = "tcp://localhost:2375"
test -z "${TESTCONTAINERS_HOST_OVERRIDE+x}"
test -z "${DOCKER_CONTEXT+x}"
test -x "$RUST_TLS_FIXTURE_BIN"
timeout --signal=TERM --kill-after=10s 180s bash -c \
  'set -eu; node qualification/testcontainers/tls.mjs "$2"; "$1" sdk "$2"' \
  _ "$RUST_TLS_FIXTURE_BIN" "$certs"
```

Both SDKs must report `localhost` as the mapped host, use the mapped HTTPS
port, accept the DNS-only `localhost` certificate with normal verification,
and reject the untrusted CA, wrong DNS name, and IP-address cases. Both
commands must exit successfully after removing their owned service container.
Record the source commit and lockfile, executable and image digests, runner and
DinD namespace identity, command outputs, and cleanup results with the run
evidence. A timeout or failed start/removal is a failed run; reconcile only
resources owned by that exact invocation before retrying, never use a broad
container cleanup. This documentation does not claim that a paired run has
passed.

For a local HTTPS-only check using the same Rust-generated bundle and Node
request logic, run:

```sh
node qualification/testcontainers-tls/selftest.mjs "$scratch/certs"
```

The self-test starts two short-lived loopback-only HTTPS servers, one on each
address family, and runs the same positive and certificate-rejection checks.
It does not start Docker or qualify the Testcontainers SDK or its mapped-port
host behavior. The shell trap removes only the temporary directory created by
the snippet.
