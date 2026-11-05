# Isolated GitHub CLI fixture

These tests require `/usr/bin/python3`, `/usr/bin/git`, Docker, the image and
Linux executables pinned in `tools.json`, and an offline Cargo registry. CI prepares the tools under
`$RUNNER_TEMP/velnor-gh-fixture-tools`. Local runs can set
`VELNOR_GH_FIXTURE_TOOLS` to an absolute directory containing the matching
`gh_linux_amd64` or `gh_linux_arm64` and `cargo-nextest_linux_amd64` or
`cargo-nextest_linux_arm64` executables. Missing inputs fail the tests.

The client, local daemon, application installation and host operating system are
trusted. The fixture rejects Docker endpoint overrides and requires a local
Unix socket, but it does not defend against a compromised host or daemon. The
worker runs with no network, using a disposable local TLS service and synthetic
credentials. It verifies the qualified executable and exercises the owned CLI.

The cache key covers tracked workspace content and the registry source packages
selected by offline Cargo metadata for all features and the native Linux target.
Untracked and ignored workspace files fail the check, except the root `target`
build directory. Inputs are checked before and after execution. This does not
prove that arbitrary build scripts cannot read other ambient inputs.
