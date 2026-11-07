# Isolated GitHub CLI fixture

These tests require `/usr/bin/python3`, `/usr/bin/git`, Docker, the image and
Linux executables pinned in `tools.json`, and an offline Cargo registry. CI prepares the tools under
`$RUNNER_TEMP/velnor-gh-fixture-tools`. Local runs can set
`VELNOR_GH_FIXTURE_TOOLS` to an absolute directory containing the matching
`gh_linux_amd64` or `gh_linux_arm64` and `cargo-nextest_linux_amd64` or
`cargo-nextest_linux_arm64` executables. Missing inputs fail the tests.

The same-base release-history test runs in a Linux x86-64 container because the
release Git bundle is pinned for that architecture. On ARM hosts, provision the
qualified `gh_linux_amd64` and `cargo-nextest_linux_amd64` files too, and enable
x86-64 container emulation. The fixture downloads and verifies the pinned Git
package when the two Git executables are not already staged.

For a local macOS ARM host, create a private directory and download the Linux
ARM64 archives for the `gh_version` and `nextest_version` in `tools.json` from
[GitHub CLI releases](https://github.com/cli/cli/releases) and
[nextest releases](https://github.com/nextest-rs/nextest/releases). Select
`gh_<version>_linux_arm64.tar.gz` and
`cargo-nextest-<version>-aarch64-unknown-linux-gnu.tar.gz`. Before extracting,
compare each `shasum -a 256` result to `gh_archive_sha256` or
`nextest_archive_sha256` under `aarch64` in `tools.json`. Extract only
`gh_<version>_linux_arm64/bin/gh` and `cargo-nextest` into the private directory,
name them `gh_linux_arm64` and `cargo-nextest_linux_arm64`, and compare their
SHA-256 values with `gh_sha256` and `nextest_sha256` in the same record. Set
`VELNOR_GH_FIXTURE_TOOLS` to that directory. Do not use the macOS executables.

Pull the exact `image` digest from `tools.json` with Docker before running the
tests, and prepare the locked Cargo registry with `cargo fetch --locked` on the
host. The worker uses the selected image and registry with `--network=none`;
missing inputs fail rather than skipping coverage. Other host architectures use
the corresponding `tools.json` record and Linux archive names.

The client, local daemon, application installation and host operating system are
trusted. The fixture rejects Docker endpoint overrides and requires a local
Unix socket, but it does not defend against a compromised host or daemon. The
worker runs with no network, using a disposable local TLS service and synthetic
credentials. It verifies the qualified executable and exercises the owned CLI.
Other processes running as the same user are also trusted: they can alter the
private validation capture while the fixture runs.

The worker builds from a private snapshot containing the working-tree bytes and
modes of tracked files, including dirty changes. Nonignored untracked files fail
the check. Ignored files and Git metadata are absent from the snapshot. A build
that requires an ignored file fails; an optional ignored build input is excluded
and must not be relied upon to qualify the test. Inputs and snapshot membership
are checked before and after execution. The cache key also covers registry source
packages selected by offline Cargo metadata for all features and the native Linux
target. This does not prove that arbitrary build scripts cannot read other
ambient inputs.
