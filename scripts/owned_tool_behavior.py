"""Closed native behavior inventory for signed owned-tool qualification."""

MISE_ABI = "mise-owned-cargo-wrapper-v1"
NATIVE_TARGETS = {("Darwin", "arm64"): "aarch64-apple-darwin",
                  ("Linux", "aarch64"): "aarch64-unknown-linux-gnu",
                  ("Linux", "arm64"): "aarch64-unknown-linux-gnu",
                  ("Linux", "x86_64"): "x86_64-unknown-linux-gnu"}


def native_host_target(host):
    if (not isinstance(host, dict) or set(host) != {"system", "machine"} or
            not all(isinstance(value, str) for value in host.values())):
        return None
    return NATIVE_TARGETS.get((host["system"], host["machine"].lower()))


MISE_CASES = (
    'cli-no-config-version-work',
    'env-no-config-version-work',
    'cli-no-config-exec-work',
    'env-no-config-exec-work',
    'cli-no-config-version-nested',
    'env-no-config-version-nested',
    'cli-no-config-exec-nested',
    'env-no-config-exec-nested',
    'exclusive-owned-work',
    'exclusive-owned-nested',
    'poison-ambient-mise-authority',
    'native-cwd-argv-exit',
    'relative-owner',
    'nonliteral-owner',
    'injection-owner',
    'missing-owner',
    'wrong-digest',
    'uppercase-digest',
    'malformed-digest',
    'empty-digest',
    'empty-owner',
    'no-config-disabled',
    'no-env-disabled',
    'no-hooks-disabled',
    'missing-MISE_OWNED_CARGO_WRAPPER',
    'missing-MISE_OWNED_CARGO_WRAPPER_SHA256',
    'missing-MISE_NO_CONFIG',
    'nonexact-true-MISE_NO_CONFIG',
    'missing-MISE_NO_ENV',
    'nonexact-true-MISE_NO_ENV',
    'missing-MISE_NO_HOOKS',
    'nonexact-true-MISE_NO_HOOKS',
    'owner-no-execute',
    'owner-writable-0755-accepted',
    'owner-noncanonical-parent-path',
    'cached-regular-shim-forged',
    'cached-symlink-shim-forged',
    'cached-foreign-mise-shim',
    'missing-shim-native-provision',
    'new-shim-exact-current-executable',
    'stale-wrapper-dir-rustc',
    'stale-wrapper-dir-mbx',
    'stale-wrapper-dir-cargo-nextest',
    'stale-wrapper-dir-other',
    'new-managed-shim-mutated-owner-target',
    'native-reprovision',
    'symlink-wrapper-directory',
    'symlink-owner',
    'world-writable-owner',
    'group-writable-owner',
    'tampered-owner',
    'native-http-install',
    'native-http-reinstall',
    'native-http-single-install',
    'native-http-wrong-checksum',
    'native-http-owned-dispatch',
    'native-http-tampered-beforeexec',
)


def valid_mise_cases(results):
    return (isinstance(results, list) and
            [case.get("case") for case in results if isinstance(case, dict)] == list(MISE_CASES) and
            all(isinstance(case, dict) and case.get("passed") is True for case in results))
