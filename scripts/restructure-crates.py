#!/usr/bin/env python3

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import argparse
import os
import posixpath
import re
import subprocess
import sys
import tomllib

ROOT_VERSION = None  # read from the root [workspace.package] version in main()
RUNNER_CRATES = (
    "velnor-resource-probe",
    "velnor-runner-attestation",
    "velnor-runner-cli",
    "velnor-runner-core",
    "velnor-runner-github",
    "velnor-runner-host",
    "velnor-runner-image-manifest",
    "velnor-runner-qualification",
)
GENERATOR_CRATES = ("velnor-gen", "velnor-model", "velnor-release", "velnor-render", "velnor-rust")
MOVES = (
    ("ci-helper/crates/velnor-ci", "crates/velnor-ci", "ci-helper"),
    *((f"generator/crates/{name}", f"crates/{name}", "generator") for name in GENERATOR_CRATES),
    ("local/crates/velnor-cache-rules", "crates/velnor-cache-rules", "local"),
    ("local/crates/velnor-local", "crates/velnor-local", "local"),
    *(
        (f"crates/velnor-runner/crates/{name}", f"crates/{name}", "crates/velnor-runner")
        for name in RUNNER_CRATES
    ),
    ("crates/test_support", "crates/velnor-test-support", None),
)
OLD_ROOTS = ("ci-helper", "generator", "local", "crates/velnor-runner")
ROOT_ENTRIES = ("Cargo.toml", "Cargo.lock", "deny.toml", "clippy.toml", ".config", "target")
OLD_ROOT_FILES = (
    "ci-helper/Cargo.toml",
    "ci-helper/Cargo.lock",
    "ci-helper/deny.toml",
    "ci-helper/.config/nextest.toml",
    "generator/Cargo.toml",
    "generator/Cargo.lock",
    "generator/deny.toml",
    "generator/.config/nextest.toml",
    "local/Cargo.toml",
    "local/Cargo.lock",
    "local/deny.toml",
    "local/.config/nextest.toml",
    "crates/velnor-runner/Cargo.toml",
    "crates/velnor-runner/Cargo.lock",
    "crates/velnor-runner/deny.toml",
    "crates/velnor-runner/clippy.toml",
    "crates/velnor-runner/.config/nextest.toml",
)
# Paths under an old root that the restructure leaves where they are (owner decision 2).
STAY_PUT_DIRS = ("generator/samples/", "local/conformance/", "local/docker/", "local/scenarios/", "local/testdata/", "ci-helper/docs/")
STAY_PUT_FILES = ("local/README.md",)
STAY_PUT_TOP_LEVEL_MD = re.compile(r"^generator/[^/]+\.md$")
ROOT_WORKSPACE_VERSION = {"ci-helper": None, "generator": "0.0.0", "local": "0.0.0", "crates/velnor-runner": "0.1.0"}
ROOT_WORKSPACE_PUBLISH_FALSE = ("generator", "local")
PINS = {
    "base64": ("0.22.1", "0.23.1"),
    "tokio": ("1.53.3", "1.54.1"),
    "serde-saphyr": ("2.1.0", "2.1.1"),
    "ureq": ("3.4.2", "3.5.2"),
    "syn": ("3.0.9", "3.0.10"),
}
KEEP_FEATURES = {
    "local/crates/velnor-local/Cargo.toml": (
        (
            'base64 = { version = "=0.22.1" }',
            'base64 = { version = "=0.23.1", default-features = false, features = ["std"] }',
        ),
    ),
}
SHIPPED_BINARIES = (
    ("velnor-ci", "velnor-ci", "ci-helper"),
    ("velnor-gen", "velnor-gen", "generator"),
    ("velnor-actions-cli", "velnor-actions-cli", "."),
    ("velnor-host", "velnor-runner-cli", "crates/velnor-runner"),
    ("velnor-local", "velnor-local", "local"),
    ("runner-attestation-helper", "velnor-runner-attestation", "crates/velnor-runner"),
    ("velnor-archive-guard", "velnor-archive-guard", "."),
)
RELEASE_TOOLS_WORKFLOW = ".github/workflows/release-rust-ci-tools.yml"
RELEASE_TOOLS_EDITS = (
    (
        'cargo build --release --locked --target "$TARGET" --manifest-path ci-helper/Cargo.toml',
        'cargo build --profile release-tools --locked --target "$TARGET" --package velnor-ci',
    ),
    (
        'cargo build --release --locked --target "$TARGET" --manifest-path generator/Cargo.toml --package velnor-gen',
        'cargo build --profile release-tools --locked --target "$TARGET" --package velnor-gen',
    ),
    (
        'cp "ci-helper/target/$TARGET/release/velnor-ci"',
        'cp "target/$TARGET/release-tools/velnor-ci"',
    ),
    (
        'cp "generator/target/$TARGET/release/velnor-gen"',
        'cp "target/$TARGET/release-tools/velnor-gen"',
    ),
)
RELEASE_PROFILE_RULE = """  - id: ci-gen-release-profile
    kind: file_content_forbidden
    paths:
      include:
        - ".github/workflows/release-rust-ci-tools.yml"
        - ".github/actions/**"
        - "scripts/**"
        - "images/**"
        - "mise.toml"
      exclude:
        - "scripts/restructure-crates.py"
        - "scripts/test-restructure-crates.py"
    pattern: 'cargo build[^\\n]*--release\\b[^\\n]*(velnor-ci|velnor-gen|ci-helper|generator/Cargo)|cargo build[^\\n]*(velnor-ci|velnor-gen|ci-helper|generator/Cargo)[^\\n]*--release\\b|release-rust-ci-tools[^\\n]*--release\\b|/release/velnor-(ci|gen)\\b|(ci-helper|generator)/target'
    level: error
    message: "velnor-ci and velnor-gen ship with the size-tuned release-tools profile: build them with `cargo build --profile release-tools -p <package>` and copy from target/<triple>/release-tools/"

"""
REPORT_TARGETS = ("aarch64-apple-darwin", "x86_64-unknown-linux-gnu")
CLAP_ENTRY = 'clap = { version = "=4.6.7", features = ["derive", "string"] }'
SKIP_PREFIXES = (
    ".git/",
    ".github/",
    "fixtures/",
    "generator/samples/",
    "docs/reviews/",
    "docs/implemented/evidence/",
    "docs/implemented/macos-scaleset/",
)
ENGINE_COUPLED = (
    "crates/velnor-actions-orchestrator/src/cache_recipes/constants.rs",
    "crates/velnor-actions-contract/src/image_candidate/closure_inventory.txt",
    "crates/velnor-actions-cli/tests/image_publication_public/source_closure.rs",
    "crates/velnor-actions-cli/tests/image_publication_public/support/promotion.rs",
)
ENGINE_COUPLED_PREFIXES = ("scripts/generator-release/",)
RUNNER_WORKSPACE_KEY = '[runner_workspace]\nroot = ""\n\n'
DEDUPE_LINES = ("scripts/archive-guard-inputs.txt",)
SKIP_FILES = (
    "scripts/restructure-crates.py",
    "scripts/test-restructure-crates.py",
    "scripts/test-native-hk-fmt.sh",
    "Cargo.lock",
)
# Changes to these need recorded owner approval, so no pass may rewrite them.
PROTECTED_FILES = ("docs/goals.md", "docs/hard-decisions.md")
BARE_CRATES = re.compile(r"(?<![\w./-])crates/(?![\w-])")
HISTORY_DOC = re.compile(r"^docs/implemented/.*(?:20\d\d-\d\d-\d\d|-v\d+\.\d+\.\d+|pr\d+)")
ENGINE_PREFIXES = ("crates/velnor-actions-", "crates/velnor-archive-guard/")
SORTED_LISTS = (
    "crates/velnor-runner-image-manifest/src/runner_family_inventory.txt",
    "crates/velnor-runner-image-manifest/src/runner_family_absences.txt",
)
TOKEN_FILES = (
    "crates/velnor-runner-image-manifest/src/inventory.rs",
    "crates/velnor-local/src/images.rs",
    "crates/velnor-local/tests/it/pairs_host.rs",
    *SORTED_LISTS,
)
CONFIG_TOML = ".velnor/config.toml"
GLOB_MAP = {
    "crates/velnor-runner/crates/*/**": ["crates/velnor-runner-*/**", "crates/velnor-resource-probe/**"],
    "local/**": ["crates/velnor-local/**", "crates/velnor-cache-rules/**"],
    "ci-helper/**": ["crates/velnor-ci/**", "ci-helper/**"],
    "generator/**": [*(f"crates/{name}/**" for name in GENERATOR_CRATES), "generator/**"],
}
RUNNER_STATUS_PATHSPECS = "".join(f'            "crates/{name}",\n' for name in RUNNER_CRATES)
SOURCE_EDITS = {
    "crates/velnor-actions-orchestrator/tests/workflow_configuration_public/maintenance_input_validation.rs": (
        ('for manifest in ["Cargo.toml", "crates/velnor-runner/Cargo.toml"] {', 'for manifest in ["Cargo.toml"] {'),
        (
            '            "crates/velnor-runner/Cargo.toml",\n            "crates/velnor-runner/Cargo.lock",\n'
            '            "crates/velnor-runner/crates/velnor-runner-attestation/src/main.rs",\n'
            '            "crates/velnor-runner/crates/velnor-runner-attestation/src/sigstore-production-tuf-root.json",\n',
            '            "Cargo.toml",\n            "Cargo.lock",\n'
            '            "crates/velnor-runner-attestation/src/main.rs",\n'
            '            "crates/velnor-runner-attestation/src/sigstore-production-tuf-root.json",\n',
        ),
    ),
    "crates/velnor-local/tests/it/pair.rs": (
        ('"crates/velnor-runner"', '"crates/velnor-runner-host"'),
        ('"crates/velnor-runner/lib.rs"', '"crates/velnor-runner-host/lib.rs"'),
    ),
    "crates/velnor-local/tests/it/pairs_host.rs": (('root.join("crates")', 'root.join("target")'),),
    "crates/velnor-local/src/images.rs": (
        ('    "--manifest-path",\n    "crates/velnor-runner/Cargo.toml",\n', ""),
        ("pub const HOST_BUILD: [&str; 10]", "pub const HOST_BUILD: [&str; 8]"),
    ),
    "crates/velnor-local/src/io/pair.rs": (
        ('            "crates/velnor-runner",\n            "images",', RUNNER_STATUS_PATHSPECS + '            "images",'),
    ),
}
NO_OLD_PATHS_RULE = """  - id: no-old-crate-paths
    kind: file_content_forbidden
    paths:
      include:
        - "scripts/**"
        - "images/**"
        - "mise.toml"
        - ".velnor/**"
        - "deny.toml"
        - ".config/**"
        - "AGENTS.md"
        - "README.md"
        - "docs/goals.md"
        - "docs/roadmap.md"
        - "docs/proposed/**"
        - "docs/content/**"
        - "local/README.md"
        - "generator/*.md"
      exclude:
        - "scripts/generator-release/**"
        - "scripts/restructure-crates.py"
        - "scripts/test-restructure-crates.py"
        - "scripts/test-native-hk-fmt.sh"
    pattern: 'ci-helper/crates|generator/crates|local/crates|crates/velnor-runner/|crates/test_support'
    level: error
    message: "Crates live in crates/velnor-* under the root workspace; use the new path (scripts/restructure-crates.py status lists the move table)"

"""
POST_EDITS = {
    ".alint.yml": (
        (
            "  - id: velnor-no-stray-npm-lock\n",
            NO_OLD_PATHS_RULE + RELEASE_PROFILE_RULE + "  - id: velnor-no-stray-npm-lock\n",
        ),
    ),
}
VERIFY_LOCAL_RUNNER_BLOCK = """RUNNER_MANIFEST="crates/velnor-runner/Cargo.toml"
if [ -f "$RUNNER_MANIFEST" ]; then
  stage runner-fmt "${MISE_EXEC[@]}" cargo fmt --manifest-path "$RUNNER_MANIFEST" --all -- --check
  stage runner-clippy "${MISE_EXEC[@]}" cargo clippy --manifest-path "$RUNNER_MANIFEST" --locked --workspace --all-targets -- -D warnings
  RUNNER_TEST_MEMBERS=""
  RUNNER_TEST_MEMBERS_OK=0
  if RUNNER_TEST_MEMBERS="$(repo_policy test-members "$RUNNER_MANIFEST" 2>"$RUNNER_TEST_MEMBER_LIST_LOG")"; then
    RUNNER_TEST_MEMBERS_OK=1
  else
    fail "runner-test-members (metadata failed; log: $RUNNER_TEST_MEMBER_LIST_LOG)"
  fi
  if [ "$RUNNER_TEST_MEMBERS_OK" -eq 1 ]; then
    if [ -z "$RUNNER_TEST_MEMBERS" ]; then
      echo "skip: runner-nextest (metadata found no test-enabled targets)"
    else
      stage runner-nextest "${NEXTEST_RUN[@]}" run --manifest-path "$RUNNER_MANIFEST" --locked --workspace --no-tests fail
    fi
  fi
  stage runner-deny mise exec "cargo-deny@0.20.2" -- cargo-deny --locked --manifest-path "$RUNNER_MANIFEST" --config "crates/velnor-runner/deny.toml" check
fi

"""
VERIFICATION_DOC_EDITS = (
    (
        "`--no-tests fail` when applicable targets exist. The nested runner workspace\nkeeps its existing workspace-wide scope, guarded by its own manifest's\nmetadata result and `--no-tests fail`. The required",
        "`--no-tests fail` when applicable targets exist. The required",
    ),
    ("It checks the root and nested workspaces, including formatting, Clippy,", "It checks the root workspace, including formatting, Clippy,"),
    (
        "complete the full relevant verification scope before finishing. The nested\nrunner is a separate Cargo workspace and root `cargo --workspace` and root\nformatting do not include it.",
        "complete the full relevant verification scope before finishing.",
    ),
    (
        "`scripts/check-freshness.sh` and covers the nested\nrunner workspace.",
        "`scripts/check-freshness.sh`.",
    ),
    (
        "in the root Cargo workspace. The nested runner workspace is excluded and must also pass `cargo fmt --manifest-path crates/velnor-runner/Cargo.toml --all -- --check`.",
        "in the root Cargo workspace, which holds every crate.",
    ),
    (
        "Run `cargo fmt --all` for root files and `cargo fmt --manifest-path crates/velnor-runner/Cargo.toml --all` for runner files.",
        "Run `cargo fmt --all`.",
    ),
    ("workspace Nextest scopes, plus the existing nested workspace scope. Cargo", "workspace Nextest scopes. Cargo"),
    (
        "## Nested Runner Workspace (`crates/velnor-runner`)\n\nThe runner controller and supervisor live in the nested virtual workspace `crates/velnor-runner`, which has its own `Cargo.toml`, `Cargo.lock`, and `deny.toml`. `scripts/verify-local.sh` queries metadata for this manifest and runs the existing workspace-wide Nextest scope with `--no-tests fail` only when applicable targets exist.\n\n```bash\ncargo fmt --manifest-path crates/velnor-runner/Cargo.toml --all -- --check\ncargo clippy --manifest-path crates/velnor-runner/Cargo.toml --locked --all-targets -- -D warnings\ncargo deny --manifest-path crates/velnor-runner/Cargo.toml check\n```\n",
        "## Runner Crates (`crates/velnor-runner-*`)\n\nThe runner controller and supervisor crates (`crates/velnor-runner-*` and `crates/velnor-resource-probe`) are members of the root workspace. They use the root `Cargo.toml`, `Cargo.lock` and `deny.toml`, and `scripts/verify-local.sh` verifies them with the root stages.\n",
    ),
)
TEXT_EDITS = {
    "scripts/verify-local.sh": ((VERIFY_LOCAL_RUNNER_BLOCK, ""),),
    "docs/implemented/ci-maintenance/repo-verification.md": VERIFICATION_DOC_EDITS,
    "scripts/test-build-owned-archive-guard.sh": (
        ('  cp -- "$repository/crates/velnor-runner/Cargo.toml" "$destination/crates/velnor-runner/"\n', ""),
    ),
    "docs/proposed/architecture.md": (
        (
            "crates/velnor-runner/             # nested runner workspace (crates/velnor-runner/Cargo.toml)",
            "crates/velnor-runner-*/           # runner packages, members of the root workspace",
        ),
    ),
    "docs/proposed/macos-scaleset-runner.md": (
        (
            "Nested virtual workspace `crates/velnor-runner/`, excluded from the root\nworkspace members. Root `cargo --workspace` does not test it. Gates enumerate\nboth workspaces.",
            "The runner packages (`crates/velnor-runner-*` and `crates/velnor-resource-probe`)\nare members of the root workspace.",
        ),
    ),
    "generator/TESTING.md": (
        ("cd generator && cargo run -p velnor-gen -- --root .. record-sample", "cargo run -p velnor-gen -- --root . record-sample"),
        ("cd generator && UPDATE_GOLDENS=1 cargo nextest run", "UPDATE_GOLDENS=1 cargo nextest run"),
    ),
    "scripts/runner-family-source-inventory.py": (
        ('    workspace = root / "crates/velnor-runner"\n', "    workspace = root\n"),
        ('native.canonical_input(dependency, root / "crates/velnor-runner", root,', "native.canonical_input(dependency, root, root,"),
    ),
    "scripts/runner-family-host-inventory.py": (('    workspace = root / "crates/velnor-runner"\n', "    workspace = root\n"),),
    ".gitignore": (("/generator/target/\n/ci-helper/target/\n/local/target/\n", ""),),
    "mise.toml": (
        (
            "".join(f'(\n  cd "$ROOT/{root}"\n  cargo fetch --locked\n)\n' for root in OLD_ROOTS[::-1][:1] + OLD_ROOTS[:3]),
            "",
        ),
    ),
    "AGENTS.md": (
        (
            "(`generator/`, `ci-helper/`, `local/`, `crates/velnor-runner`)",
            "(`crates/velnor-gen`, `crates/velnor-model`, `crates/velnor-release`, `crates/velnor-render`, `crates/velnor-rust`, `crates/velnor-ci`, `crates/velnor-local`, `crates/velnor-cache-rules`, `crates/velnor-runner-*`, `crates/velnor-resource-probe`)",
        ),
        (
            "(`crates/` except `crates/velnor-runner`, `.velnor/`,",
            "(`crates/velnor-actions-*`, `crates/velnor-archive-guard`, `.velnor/`,",
        ),
    ),
    "docs/roadmap.md": (
        ("different top-level folders (`generator/`, `ci-helper/`, `local/`, `crates/`)", "different crates"),
        (
            "is in the nextest config of two workspaces: `generator/` and `ci-helper/`. `local/` and `crates/velnor-runner/` set none.",
            "is a `.config/nextest.toml` override for `velnor-ci`, `velnor-gen`, `velnor-model`, `velnor-release`, `velnor-render` and `velnor-rust`. The other crates set none.",
        ),
    ),
    ".alint.yml": (
        ('        - "crates/test_support/**"\n', '        - "crates/velnor-test-support/**"\n'),
        (
            "".join(
                f'        - "{prefix}/*/{tail}"\n'
                for prefix, tails in (
                    ("crates/velnor-runner/crates", ("src/**", "tests/**", "benches/**", "examples/**", "build.rs")),
                    ("ci-helper/crates", ("src/**", "tests/**")),
                    ("generator/crates", ("src/**", "tests/**")),
                )
                for tail in tails
            ),
            "",
        ),
        ('        - "local/crates/*/src/**"\n        - "local/crates/*/tests/**"\n', ""),
        (
            "        - Cargo.lock\n        - crates/velnor-runner/Cargo.lock\n        - ci-helper/Cargo.lock\n        - generator/Cargo.lock\n        - local/Cargo.lock\n",
            "        - Cargo.lock\n",
        ),
        (
            "Only the root lock, crates/velnor-runner/Cargo.lock, ci-helper/Cargo.lock, generator/Cargo.lock and local/Cargo.lock are resolved.",
            "Only the root lock is resolved.",
        ),
        (
            '        - "crates/velnor-actions-*/src/lib.rs"\n        - "crates/velnor-actions-*/src/main.rs"\n'
            '        - "crates/velnor-archive-guard/src/lib.rs"\n        - "crates/velnor-archive-guard/src/main.rs"\n'
            + "".join(
                f'        - "{prefix}/*/src/{name}"\n'
                for prefix in ("crates/velnor-runner/crates", "ci-helper/crates", "generator/crates", "local/crates")
                for name in ("lib.rs", "main.rs")
            ),
            '        - "crates/*/src/lib.rs"\n        - "crates/*/src/main.rs"\n',
        ),
        (
            '        - "ci-helper/crates/*/src/**/*.rs"\n        - "generator/crates/*/src/**/*.rs"\n        - "local/crates/*/src/**/*.rs"\n',
            "",
        ),
    ),
}
COUPLED_EDITS = {
    "scripts/generator-release/source-qualification.sh": (
        (
            "manifests=(Cargo.toml crates/velnor-runner/Cargo.toml)\nlocks=(Cargo.lock crates/velnor-runner/Cargo.lock)\n"
            "fetch_scope=root-and-runner\n"
            + "".join(
                f'if [ -f "$root/{name}/Cargo.toml" ]; then\n  manifests+=({name}/Cargo.toml)\n  locks+=({name}/Cargo.lock)\n'
                f'  fetch_scope="$fetch_scope-and-{name}"\nfi\n'
                for name in ("ci-helper", "generator", "local")
            ),
            "manifests=(Cargo.toml)\nlocks=(Cargo.lock)\nfetch_scope=root\n",
        ),
    ),
    "scripts/generator-release/stable-source-consumer.sh": (
        (
            "for manifest in Cargo.toml crates/velnor-runner/Cargo.toml; do\n"
            '  mise --no-config --no-env --no-hooks exec "rust@$VELNOR_RUST_VERSION" -- cargo metadata --locked --offline --all-features --format-version=1 --manifest-path "$GITHUB_WORKSPACE/$manifest" > /dev/null\n'
            "done\n",
            'mise --no-config --no-env --no-hooks exec "rust@$VELNOR_RUST_VERSION" -- cargo metadata --locked --offline --all-features --format-version=1 --manifest-path "$GITHUB_WORKSPACE/Cargo.toml" > /dev/null\n',
        ),
    ),
    "scripts/generator-release/qualification-goldens.sh": (
        (
            'for manifest in "$repo/Cargo.toml" "$repo/crates/velnor-runner/Cargo.toml" "$repo/ci-helper/Cargo.toml" "$repo/generator/Cargo.toml" "$repo/local/Cargo.toml"; do',
            'for manifest in "$repo/Cargo.toml"; do',
        ),
    ),
}
TOKEN = re.compile(
    r"(?<![\w.])(?<!\w-)((?:crates/velnor-runner|ci-helper|generator|local|crates/test_support)(?![\w-])(?:/[\w.@+*?{}\[\]-]*)*)"
)
PROSE_BARE = re.compile(r"(?<![\w./-])crates/velnor-runner(?:/crates/\*)?(?![\w*-]|/[\w*])")
LITERAL = re.compile(r'"((?:[^"\\\n]|\\.)*)"')
DOTDOT = re.compile(r"(?:^|/)\.\.(?:/|$)")
LITERAL_CONTEXT = re.compile(
    r"(?:\.join\(|\.push\(|Path::new\(|PathBuf::from\(|include_str!\(|include_bytes!\(|include!\(|"
    r"read_to_string\(|fs::read\(|path\s*=\s*|format!\(|concat!\(|CARGO_MANIFEST_DIR\"\)\s*,)\s*$"
)
FILE_CONTEXT = re.compile(r"(?:include_str!|include_bytes!|include!|path\s*=)\s*\(?\s*$")
PATH_DEP = re.compile(r'(\bpath\s*=\s*")([^"]+)(")')
ROOT_LINTS_ANCHOR = 'let_underscore_must_use = "deny"\n'
# The restructure only moves code, so it does not rewrite the `let _ = ...;` statements the root lint rejects
# in the crates that did not have it before. The lint is allowed for the whole workspace instead.
LET_UNDERSCORE_ALLOW = (
    "# Allowed during the restructure; the `let _ =` statements are rewritten in a follow-up PR.\n"
    'let_underscore_must_use = "allow"\n'
)
RELEASE_PROFILES = """
[profile.dev.package.turso_core]
opt-level = 1

[profile.release-tools]
inherits = "release"
opt-level = "s"
lto = "thin"
strip = true
panic = "abort"
"""
NEXTEST_HEAD = """# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# Nextest profiles (Gate 0 skeleton).
# Empty-run behavior is enforced via CLI `--no-tests fail` (contract §4);
# nextest has no profile-level equivalent, so it is not set here.
experimental = ["wrapper-scripts"]

[scripts.wrapper.local-docker-ci]
command = ["sh", "-c", 'if [ "${GITHUB_ACTIONS:-}" = true ] && [ "${VELNOR_TEST_DOCKER_HOST+x}" != x ]; then export VELNOR_TEST_DOCKER_HOST=unix:///var/run/docker.sock; fi; exec "$@"', "local-docker-ci"]

[scripts.wrapper.no-programs]
command = ["/usr/bin/env", "PATH=/nonexistent"]

[test-groups]
docker-networks = { max-threads = 4 }
velnor-ci-serial = { max-threads = 1 }

[profile.ci]
retries = 0
test-threads = "num-cpus"
status-level = "all"
final-status-level = "flaky"

[[profile.default.scripts]]
filter = "package(=velnor-gen) or package(=velnor-model) or package(=velnor-release) or package(=velnor-render) or package(=velnor-rust)"
list-wrapper = "no-programs"
run-wrapper = "no-programs"

[[profile.ci.scripts]]
filter = "package(=velnor-gen) or package(=velnor-model) or package(=velnor-release) or package(=velnor-render) or package(=velnor-rust)"
list-wrapper = "no-programs"
run-wrapper = "no-programs"

[[profile.ci.scripts]]
filter = 'package(=velnor-runner-host) and (binary(=local_pair_public) or binary(=local_host_public))'
platform = 'cfg(target_os = "linux")'
run-wrapper = 'local-docker-ci'

[[profile.ci.scripts]]
filter = 'package(=velnor-local) and binary(=it) and test(removal_audit::)'
platform = 'cfg(target_os = "linux")'
run-wrapper = 'local-docker-ci'
"""
NEXTEST_TAIL = """[[profile.default.overrides]]
filter = "binary(local_host_public) | binary(local_pair_public)"
test-group = "docker-networks"
success-output = "immediate"

[[profile.default.overrides]]
filter = 'package(=velnor-local) and binary(=it) and test(removal_audit::)'
success-output = "immediate"

[[profile.default.overrides]]
filter = "package(=velnor-ci)"
test-group = "velnor-ci-serial"

[[profile.ci.overrides]]
filter = "package(=velnor-ci)"
test-group = "velnor-ci-serial"

[[profile.default.overrides]]
filter = "package(=velnor-ci) or package(=velnor-gen) or package(=velnor-model) or package(=velnor-release) or package(=velnor-render) or package(=velnor-rust)"
slow-timeout = { period = "2s", terminate-after = 1 }
"""
DENY_EXCEPTION_NOTE = (
    "# CDLA-Permissive-2.0 is accepted for webpki-roots only (the Mozilla CA data it ships).\n"
    "# It is a per-crate exception on purpose: do not add it to the global `allow` list above.\n"
)
DENY_EXCEPTIONS = (("CDLA-Permissive-2.0", ("webpki-roots",)),)

ROOT = None
ENGINE_NEUTRAL = False
UNRESOLVED = []


def git(*args, check=True):
    result = subprocess.run(["git", "-C", ROOT, *args], capture_output=True, text=True)
    if check and result.returncode != 0:
        sys.exit(f"git {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout


def tracked():
    return [line for line in git("ls-files", "-z").split("\0") if line]


def read(path):
    with open(os.path.join(ROOT, path), encoding="utf-8") as handle:
        return handle.read()


def write(path, text):
    with open(os.path.join(ROOT, path), "w", encoding="utf-8", newline="") as handle:
        handle.write(text)


def exists(path):
    return os.path.exists(os.path.join(ROOT, path))


def map_path(path):
    for old, new, _ in sorted(MOVES, key=lambda row: -len(row[0])):
        if path == old or path.startswith(old + "/"):
            return new + path[len(old) :]
    for root in OLD_ROOTS:
        for entry in ROOT_ENTRIES:
            prefix = f"{root}/{entry}"
            if path == prefix or path.startswith(prefix + "/"):
                return entry + path[len(prefix) :]
    return path


def map_directory(path):
    return "." if path == "crates/velnor-runner" else map_path(path)


def old_location(path):
    for old, new, _ in MOVES:
        if path == new or path.startswith(new + "/"):
            return old + path[len(new) :]
    return path


def is_engine(path):
    return path.startswith(ENGINE_PREFIXES)


def skipped(path):
    return path in SKIP_FILES or path in PROTECTED_FILES or path.startswith(SKIP_PREFIXES) or bool(HISTORY_DOC.match(path))


def protected_note():
    for path in PROTECTED_FILES:
        if not exists(path):
            continue
        text = read(path)
        stale = sorted({m.group(1) for m in TOKEN.finditer(text) if "/" in m.group(1) or text[m.start() - 1 : m.start()] == "`"})
        if BARE_CRATES.search(text):
            stale.append("crates/ (bare directory reference; the crates now include the generator, runner and local crates)")
        if stale:
            print(f"NOTE: {path} is protected and was not rewritten; update by hand. Stale references: {', '.join(stale)}")
        else:
            print(f"NOTE: {path} is protected and was not rewritten; no stale references found")


def tree_state(files):
    dirs = {""}
    for path in files:
        directory = posixpath.dirname(path)
        while directory and directory not in dirs:
            dirs.add(directory)
            directory = posixpath.dirname(directory)
    return set(files), dirs


def crate_dir(path, files):
    directory = posixpath.dirname(path)
    while directory:
        if f"{directory}/Cargo.toml" in files:
            return directory
        directory = posixpath.dirname(directory)
    return ""


def rewrite_tokens(text):
    def replace(match):
        token = match.group(1)
        tail = ""
        while token.endswith("}") and token.count("{") < token.count("}"):
            token, tail = token[:-1], "}" + tail
        return map_path(token) + tail

    return TOKEN.sub(replace, text)


def rewrite_globs(text):
    for old, news in GLOB_MAP.items():
        text = re.sub(
            r'^(\s*)"' + re.escape(old) + r'",?$',
            lambda m, news=news: ",\n".join(f'{m.group(1)}"{new}"' for new in news) + ",",
            text,
            flags=re.M,
        )
        text = text.replace(f'["{old}"]', "[" + ", ".join(f'"{new}"' for new in news) + "]")
        text = text.replace(f'["{old}", ', "[" + ", ".join(f'"{new}"' for new in news) + ", ")
    return text


def dedupe_array_lines(text):
    out = []
    seen = None
    for line in text.split("\n"):
        stripped = line.strip()
        if stripped.endswith("= ["):
            seen = set()
        elif stripped.startswith("]"):
            seen = None
        elif seen is not None and stripped.startswith('"'):
            if stripped in seen:
                continue
            seen.add(stripped)
        out.append(line)
    return "\n".join(out)


def rewrite_literals(path, text, files, dirs, mode):
    if mode == "apply":
        old_file, new_file = path, map_path(path)
        old_crate = crate_dir(old_file, files)
        new_crate = map_path(old_crate) if old_crate else ""
    else:
        new_file, old_file = path, old_location(path)
        new_crate = crate_dir(new_file, files)
        old_crate = old_location(new_crate) if new_crate else ""
    unresolved = []

    def replace(match):
        literal = match.group(1)
        if literal == ".." or not DOTDOT.search(literal):
            return match.group(0)
        before = re.sub(r"\s+", " ", text[max(0, match.start() - 60) : match.start()])
        in_file = bool(FILE_CONTEXT.search(before))
        if not literal.startswith("{}/") and not LITERAL_CONTEXT.search(before):
            return match.group(0)
        prefix, body = "", literal
        for lead in ("{}/", "/"):
            if body.startswith(lead):
                prefix, body = lead, body[len(lead) :]
                break
        if not (body == ".." or body.startswith("../")):
            return match.group(0)
        trailing = "/" if body.endswith("/") and body != "/" else ""
        old_base = posixpath.dirname(old_file) if in_file else old_crate
        new_base = posixpath.dirname(new_file) if in_file else new_crate
        if mode == "fixup":
            current = posixpath.normpath(posixpath.join(new_base, body))
            if current == ".":
                current = ""
            if not current.startswith("..") and (current in files or current in dirs):
                return match.group(0)
        target = posixpath.normpath(posixpath.join(old_base, body))
        if target.startswith(".."):
            return match.group(0)
        if target == ".":
            target = ""
        mapped = map_directory(target)
        if mapped == target and old_base == new_base:
            return match.group(0)
        formatted = "{" in literal
        if mode == "apply":
            known = target in files or target in dirs or (formatted and posixpath.dirname(target) in dirs)
        else:
            known = mapped in files or mapped in dirs or (formatted and posixpath.dirname(mapped) in dirs)
        if not known and mapped == target:
            if in_file:
                unresolved.append((old_file, literal))
            return match.group(0)
        relative = posixpath.relpath(mapped or ".", new_base or ".")
        return f'"{prefix}{relative}{trailing if relative != "." else ""}"'

    rewritten = LITERAL.sub(replace, text)
    return rewritten, unresolved


def apply_pins(text):
    for name, (old, new) in PINS.items():
        text = re.sub(
            r"^(\s*" + re.escape(name) + r'\b[^\n]*?"=)' + re.escape(old) + '"',
            lambda m, new=new: m.group(1) + new + '"',
            text,
            flags=re.M,
        )
    return text


def rewrite_dep_paths(text, old_dir, new_dir):
    def replace(match):
        target = posixpath.normpath(posixpath.join(old_dir, match.group(2)))
        return match.group(1) + posixpath.relpath(map_path(target), new_dir or ".") + match.group(3)

    return PATH_DEP.sub(replace, text)


def fixup_dep_paths(path, text, files, dirs):
    """Re-express `path =` values written against the manifest's OLD location so they resolve from the new one."""
    new_dir = posixpath.dirname(path)
    old_dir = posixpath.dirname(old_location(path))

    def replace(match):
        value = match.group(2)
        here = posixpath.normpath(posixpath.join(new_dir, value))
        if here == ".":
            here = ""
        if not here.startswith("..") and (here in files or here in dirs):
            return match.group(0)
        target = posixpath.normpath(posixpath.join(old_dir, value))
        mapped = map_path(target)
        if target.startswith("..") or not (mapped in files or mapped in dirs):
            UNRESOLVED.append(f"{path}: path = {value!r}")
            return match.group(0)
        return match.group(1) + posixpath.relpath(mapped or ".", new_dir or ".") + match.group(3)

    return PATH_DEP.sub(replace, text)


def rewrite_manifest(path, text):
    old_dir = posixpath.dirname(path)
    new_dir = posixpath.dirname(map_path(path))
    text = rewrite_dep_paths(text, old_dir, new_dir)
    for old, new in KEEP_FEATURES.get(path, ()):
        if old not in text:
            sys.exit(f"feature-keeping anchor missing in {path}: {old!r}")
        text = text.replace(old, new, 1)
    row = next((row for row in MOVES if path.startswith(row[0] + "/")), None)
    if row is not None and row[2] is not None:
        version = ROOT_WORKSPACE_VERSION[row[2]]
        if version and version != ROOT_VERSION:
            text = re.sub(r"^version\.workspace = true$", f'version = "{version}"', text, flags=re.M)
        if row[2] in ROOT_WORKSPACE_PUBLISH_FALSE:
            text = re.sub(r"^publish\.workspace = true$", "publish = false", text, flags=re.M)
    return apply_pins(text)


def apply_text_edits(path, text):
    for table in (TEXT_EDITS, SOURCE_EDITS):
        for old, new in table.get(path) or table.get(map_path(path)) or ():
            if old not in text:
                sys.exit(f"edit anchor missing in {path}: {old[:80]!r}")
            text = text.replace(old, new)
    return text


def in_moved_crate(path):
    return any(path.startswith(old + "/") or path.startswith(new + "/") for old, new, _ in MOVES)


def is_coupled(path):
    return path in ENGINE_COUPLED or path.startswith(ENGINE_COUPLED_PREFIXES)


def dedupe_blocks(text):
    out = []
    seen = None
    for line in text.splitlines(keepends=True):
        if re.search(r"const \w*INPUTS: &\[&str\] = &\[$", line.rstrip("\n")):
            seen = set()
        elif line.startswith("];"):
            seen = None
        elif seen is not None and line.lstrip().startswith('"'):
            if line in seen:
                continue
            seen.add(line)
        out.append(line)
    return "".join(out)


def transform_coupled(path, text, mode):
    for old, new in COUPLED_EDITS.get(path, ()):
        text = text.replace(old, new)
    text = rewrite_tokens(text)
    if path.endswith("closure_inventory.txt"):
        text = "\n".join(sorted({line for line in text.split("\n") if line})) + "\n"
    if path.endswith(".rs"):
        text = dedupe_blocks(text)
    return text


def transform(path, text, files, dirs, mode):
    if is_coupled(path):
        return transform_coupled(path, text, mode) if ENGINE_NEUTRAL else text
    is_manifest = path.endswith("/Cargo.toml") and (
        is_engine(path) or any(path.startswith(old + "/") for old, _, _ in MOVES)
    )
    if mode == "fixup" and (path == "Cargo.toml" or path.endswith("/Cargo.toml")):
        text = fixup_dep_paths(path, text, files, dirs)
    if is_manifest:
        return rewrite_manifest(path, text) if mode == "apply" else text
    if mode == "apply":
        text = apply_text_edits(path, text)
    if path.endswith(".rs"):
        text, unresolved = rewrite_literals(path, text, files, dirs, mode)
        UNRESOLVED.extend(f"{path}: {literal}" for _, literal in unresolved)
        if path in TOKEN_FILES or map_path(path) in TOKEN_FILES:
            text = rewrite_tokens(text)
        return text
    if is_engine(path) or (in_moved_crate(path) and map_path(path) not in TOKEN_FILES):
        return text
    if path == ".alint.yml" and mode != "apply":
        return text
    text = rewrite_tokens(text)
    if mode == "apply":
        for old, new in POST_EDITS.get(path, ()):
            if old not in text:
                sys.exit(f"post edit anchor missing in {path}")
            text = text.replace(old, new, 1)
    if path.endswith((".md", ".mdx")):
        text = PROSE_BARE.sub("crates/velnor-runner-*", text)
    if path == CONFIG_TOML:
        text = dedupe_array_lines(rewrite_globs(text))
        if ENGINE_NEUTRAL and "[runner_workspace]" not in text:
            text = text.replace("[stacks]\n", RUNNER_WORKSPACE_KEY + "[stacks]\n", 1)
    if path in DEDUPE_LINES:
        seen = set()
        text = "".join(line for line in text.splitlines(keepends=True) if not (line in seen or seen.add(line)))
    if map_path(path) in SORTED_LISTS:
        text = "\n".join(sorted({line for line in text.split("\n") if line})) + "\n"
    return text


def section_lines(text, header):
    out, inside = [], False
    for line in text.split("\n"):
        if line.strip() == header:
            inside = True
        elif inside and line.startswith("["):
            break
        elif inside and line.strip() and not line.lstrip().startswith("#"):
            out.append(line)
    return out


def entry_name(line):
    return line.split("=", 1)[0].strip()


def build_root_manifest(root_text, old_texts):
    base_entries = {entry_name(line): line for line in section_lines(root_text, "[workspace.dependencies]")}
    merged = dict(base_entries)
    for old_root in ("crates/velnor-runner", "generator", "ci-helper", "local"):
        for line in section_lines(old_texts[old_root], "[workspace.dependencies]"):
            name = entry_name(line)
            line = apply_pins(rewrite_dep_paths(line, old_root, ""))
            if name == "clap":
                line = CLAP_ENTRY
            if name in merged and merged[name] != line:
                if name != "clap":
                    sys.exit(f"workspace dependency conflict without a rule: {merged[name]} vs {line}")
            merged[name] = line
    runner = tomllib.loads(old_texts["crates/velnor-runner"])
    base = tomllib.loads(root_text)
    for table in ("rust", "clippy", "rustdoc"):
        if runner["workspace"]["lints"][table] != base["workspace"]["lints"][table]:
            sys.exit(f"workspace.lints.{table} differs between the root and the runner workspace")
    for key in ("patch", "replace"):
        if key in base or any(key in tomllib.loads(text) for text in old_texts.values()):
            sys.exit(f"[{key}] tables are not supported by this codemod")
    members = sorted(
        {*base["workspace"]["members"], *(new for _, new, _ in MOVES if new != "crates/velnor-test-support")}
    )
    listing = "".join(f'    "{member}",\n' for member in members)
    text = re.sub(
        r'^\[workspace\]\n.*?^resolver = "3"\n',
        lambda _: f'[workspace]\nmembers = [\n{listing}]\nresolver = "3"\n',
        root_text,
        count=1,
        flags=re.M | re.S,
    )
    for name, line in base_entries.items():
        text = text.replace(line, merged[name], 1)
    additions = [line for name, line in merged.items() if name not in base_entries]
    text = text.replace("\n[workspace.package]", "\n".join(additions) + "\n\n[workspace.package]", 1)
    if ROOT_LINTS_ANCHOR not in text:
        sys.exit("root lint anchor missing")
    text = text.replace(ROOT_LINTS_ANCHOR, LET_UNDERSCORE_ALLOW + 'missing_errors_doc = "allow"\n', 1)
    return text.rstrip("\n") + "\n" + RELEASE_PROFILES


def merged_nextest(root_text, generator_text, ci_text):
    root_overrides = "[[profile.ci.overrides]]" + root_text.split("[[profile.ci.overrides]]", 1)[1]
    generator_overrides = "[[profile.ci.overrides]]" + generator_text.split("[[profile.ci.overrides]]", 1)[1]
    ci_blocks = re.findall(r"\[\[profile\.(?:default|ci)\.overrides\]\]\n(?:[^\[\n].*\n?)+", ci_text)
    ci_blocks = [
        re.sub(r"^filter = '([^']*)'", r"filter = 'package(=velnor-ci) and \1'", block, flags=re.M)
        for block in ci_blocks
    ]
    parts = [NEXTEST_HEAD, root_overrides.rstrip("\n") + "\n", generator_overrides.rstrip("\n") + "\n"]
    parts.append("".join(block.rstrip("\n") + "\n\n" for block in ci_blocks).rstrip("\n") + "\n")
    parts.append(NEXTEST_TAIL)
    return "\n".join(parts)


def merged_deny(root_text):
    out = root_text
    for license_name, crates in DENY_EXCEPTIONS:
        for crate in crates:
            out = out.rstrip("\n") + f"\n\n{DENY_EXCEPTION_NOTE}[[licenses.exceptions]]\nallow = [\"{license_name}\"]\ncrate = \"{crate}\"\n"
    return out


def compat_key(name, version):
    parts = version.split("+")[0].split("-")[0].split(".")
    numbers = [int(part) for part in parts[:3]]
    if numbers[0] > 0:
        return (name, numbers[0])
    if numbers[1] > 0:
        return (name, 0, numbers[1])
    return (name, 0, 0, numbers[2])


def version_tuple(version):
    return tuple(int(part) for part in version.split("+")[0].split("-")[0].split("."))


def union_lock(texts):
    packages = {}
    for text in texts:
        data = tomllib.loads(text)
        versions = {}
        for package in data["package"]:
            versions.setdefault(package["name"], []).append(package["version"])
        for package in data["package"]:
            if "source" not in package:
                continue
            dependencies = []
            for entry in package.get("dependencies", []):
                parts = entry.split(" ")
                if len(parts) == 1:
                    if len(versions.get(parts[0], [])) != 1:
                        sys.exit(f"ambiguous lock dependency {entry} in {package['name']}")
                    entry = f"{parts[0]} {versions[parts[0]][0]}"
                dependencies.append(entry)
            record = dict(package)
            record["dependencies"] = dependencies
            packages.setdefault((package["name"], package["version"]), record)
    best = {}
    for name, version in packages:
        key = compat_key(name, version)
        if key not in best or version_tuple(version) > version_tuple(best[key]):
            best[key] = version
    survivors = {}
    for (name, version), record in packages.items():
        if best[compat_key(name, version)] != version:
            continue
        mapped = []
        for entry in record["dependencies"]:
            dep_name, dep_version = entry.split(" ")[:2]
            mapped.append(f"{dep_name} {best[compat_key(dep_name, dep_version)]}")
        record["dependencies"] = sorted(set(mapped))
        survivors[(name, version)] = record
    return survivors


def format_lock(packages):
    out = ["# This file is automatically @generated by Cargo.", "# It is not intended for manual editing.", "version = 4", ""]
    for key in sorted(packages):
        package = packages[key]
        out += ["[[package]]", f'name = "{package["name"]}"', f'version = "{package["version"]}"']
        out.append(f'source = "{package["source"]}"')
        if "checksum" in package:
            out.append(f'checksum = "{package["checksum"]}"')
        if package["dependencies"]:
            out.append("dependencies = [")
            out += [f' "{dep}",' for dep in sorted(package["dependencies"])]
            out.append("]")
        out.append("")
    return "\n".join(out)


def lock_versions(text):
    return {(p["name"], p["version"]) for p in tomllib.loads(text)["package"] if "source" in p}


def highest_per_range(versions):
    best = {}
    for name, version in versions:
        key = compat_key(name, version)
        if key not in best or version_tuple(version) > version_tuple(best[key]):
            best[key] = version
    return best


def lowered_versions(old_lock_texts, result):
    old_best = highest_per_range(set().union(*(lock_versions(text) for text in old_lock_texts)))
    new_best = highest_per_range(result)
    return sorted(
        (key[0], old_best[key], new_best[key])
        for key in old_best
        if key in new_best and version_tuple(new_best[key]) < version_tuple(old_best[key])
    )


def merge_locks(old_lock_texts):
    packages = union_lock(old_lock_texts)
    write("Cargo.lock", format_lock(packages))
    update = subprocess.run(["cargo", "update", "--workspace"], cwd=ROOT, capture_output=True, text=True)
    if update.returncode != 0:
        sys.exit("cargo update --workspace failed:\n" + update.stderr)
    result = lock_versions(read("Cargo.lock"))
    lowered = lowered_versions(old_lock_texts, result)
    if lowered:
        sys.exit(
            "the merged lock is lower than an old lock for: "
            + ", ".join(f"{name} {old} -> {new}" for name, old, new in lowered)
            + "; an exact `=` requirement pins the lower version, add the pair to PINS"
        )
    print(f"lock: {len(result)} registry packages; not in any old lock: {sorted(result - set(packages)) or 'none'}")


TREE_LINE = re.compile(r"^(\S+) v(\d[^\s]*)(?: \((?!proc-macro|\*)([^)]*)\))?")


def tree_versions(cwd, package, target):
    out = subprocess.run(
        ["cargo", "tree", "--locked", "-p", package, "--prefix", "none", "-e", "normal,build", "--target", target],
        cwd=cwd,
        capture_output=True,
        text=True,
    )
    if out.returncode != 0:
        sys.exit(f"cargo tree -p {package} failed in {cwd}:\n{out.stderr}")
    versions = {}
    for line in out.stdout.splitlines():
        match = TREE_LINE.match(line)
        if match and not match.group(3):
            versions.setdefault(match.group(1), set()).add(match.group(2))
    return versions


def collect_trees(old_layout):
    return {
        (label, target): tree_versions(os.path.join(ROOT, root if old_layout else "."), package, target)
        for label, package, root in SHIPPED_BINARIES
        for target in REPORT_TARGETS
    }


def version_report(before, after):
    lines = [
        "Resolved version changes per shipped binary (`cargo tree --locked -p <package> --prefix none -e normal,build --target <target>`, old workspace at the base versus the root workspace):",
        "",
    ]
    total = 0
    lowered = set()
    for label, package, _ in SHIPPED_BINARIES:
        for target in REPORT_TARGETS:
            old = before[(label, target)]
            new = after[(label, target)]
            rows = []
            for name in sorted(set(old) | set(new)):
                if old.get(name) != new.get(name):
                    rows.append(
                        f"| {name} | {', '.join(sorted(old.get(name, ()))) or '-'} | {', '.join(sorted(new.get(name, ()))) or '-'} |"
                    )
            total += len(rows)
            for name in set(old) & set(new):
                if max(map(version_tuple, new[name])) < max(map(version_tuple, old[name])):
                    lowered.add(f"{label}: {name} {max(old[name])} -> {max(new[name])}")
            lines += [f"### {label} (`-p {package}`, {target}): {len(rows)} changes", ""]
            if rows:
                lines += ["| crate | before | after |", "|---|---|---|", *rows, ""]
    lines.append(f"Total: {total} version changes (rows per binary and target).")
    lines.append("Downgrades: " + ("; ".join(sorted(lowered)) if lowered else "none") + ".")
    return "\n".join(lines) + "\n"


def append_secret_baselines():
    for path in (".gitleaksignore", ".betterleaksignore"):
        lines = read(path).split("\n")
        have = set(lines)
        added = []
        for line in lines:
            if line.startswith("#") or not line:
                continue
            parts = line.split(":")
            rest = parts[1:] if re.fullmatch(r"[0-9a-f]{40}", parts[0]) else parts
            if not rest or map_path(rest[0]) == rest[0]:
                continue
            entry = ":".join([map_path(rest[0]), *rest[1:]])
            if entry not in have:
                have.add(entry)
                added.append(entry)
        if added:
            write(path, "\n".join(lines).rstrip("\n") + "\n\n# Paths after the crate restructure\n" + "\n".join(added) + "\n")
    text = read(".kingfisher-baseline.yml")
    blocks = re.split(r"(?m)^(?=  - path: )", text)
    existing = set(blocks)
    extra = []
    for block in blocks[1:]:
        match = re.match(r"  - path: (\./)?(\S+)\n", block)
        if match and map_path(match.group(2)) != match.group(2):
            copy = block.replace(match.group(0), f"  - path: {match.group(1) or ''}{map_path(match.group(2))}\n", 1)
            if copy not in existing:
                existing.add(copy)
                extra.append(copy)
    if extra:
        write(".kingfisher-baseline.yml", text.rstrip("\n") + "\n" + "".join(extra))


def stay_put(path):
    return path in STAY_PUT_FILES or path.startswith(STAY_PUT_DIRS) or bool(STAY_PUT_TOP_LEVEL_MD.match(path))


def stranded_files():
    """Tracked files still under an old crate directory of the move table, with the path each one moves to."""
    return {path: map_path(path) for path in tracked() if any(path.startswith(old + "/") for old, _, _ in MOVES)}


def leftover_files():
    """Tracked files under an old root that are neither crate files, old root files nor stay-put paths."""
    roots = tuple(root + "/" for root in OLD_ROOTS)
    moved = stranded_files()
    return [
        path
        for path in tracked()
        if path.startswith(roots) and path not in moved and path not in OLD_ROOT_FILES and not stay_put(path)
    ]


def move_stranded():
    """git mv every stranded file; returns the new paths and the files that could not move."""
    moved, blocked = [], []
    for old, new in sorted(stranded_files().items()):
        if exists(new):
            blocked.append(f"{old} -> {new} (destination already exists)")
            continue
        os.makedirs(os.path.dirname(os.path.join(ROOT, new)), exist_ok=True)
        git("mv", old, new)
        moved.append(new)
        prune_empty(posixpath.dirname(old))
    return moved, blocked


def blob(revision, path):
    result = subprocess.run(["git", "-C", ROOT, "show", f"{revision}:{path}"], capture_output=True, text=True)
    return result.stdout if result.returncode == 0 else None


def is_ancestor(ancestor, descendant):
    return subprocess.run(["git", "-C", ROOT, "merge-base", "--is-ancestor", ancestor, descendant]).returncode == 0


def branch_view(base):
    """(fork point, branch tip) for the branch's own commits, whether or not the restructure is merged yet."""
    target = git("rev-parse", base).strip()
    merge = None
    for commit in git("rev-list", "--first-parent", "HEAD").split():
        if not is_ancestor(target, commit):
            break
        merge = commit
    if merge is None:
        return git("merge-base", "HEAD", base).strip(), "HEAD"
    parents = git("rev-list", "--parents", "-n", "1", merge).split()[1:]
    if merge == target or len(parents) < 2:
        return None
    return git("merge-base", parents[0], base).strip(), parents[0]


LOCK_ADVICE = (
    "Do not merge lockfiles. Add the dependency change to the manifest that owns it, then run `cargo update -w` "
    "at the root to refresh Cargo.lock."
)


def port_advice(path):
    target = map_path(path)
    name = posixpath.basename(path)
    if name == "Cargo.lock":
        return LOCK_ADVICE
    advice = f"Port the hunk below by hand into root `{target}`" + ("" if exists(target) else " (it does not exist; the restructure drops this file)") + "."
    if name == "nextest.toml" and path.startswith("ci-helper/"):
        advice += " `apply` merged ci-helper override blocks there with `package(=velnor-ci) and` in front of each filter."
    if name == "Cargo.toml":
        advice += " Only [workspace.dependencies], [workspace.lints] and profile entries belong in the root manifest."
    return advice


def deleted_file_edits(base):
    """Edits the branch made to files the restructure deleted or merged; they are not carried over by the merge."""
    view = branch_view(base)
    if view is None:
        return []
    fork, tip = view
    reports = []
    for path in OLD_ROOT_FILES:
        before, after = blob(fork, path), blob(tip, path)
        if before == after:
            if exists(path):
                git("rm", "-q", "-f", path)
                prune_empty(posixpath.dirname(path))
            continue
        lines = [f"EDITED DELETED FILE {path} (restructure target: {map_path(path)})", f"  {port_advice(path)}"]
        if posixpath.basename(path) != "Cargo.lock":
            lines += ["  " + line for line in git("diff", fork, tip, "--", path).split("\n")]
        lines.append(f"  After porting: git rm -f {path}")
        reports.append("\n".join(lines))
    return reports


def status_report():
    old_manifests = [root for root in OLD_ROOTS if exists(f"{root}/Cargo.toml")]
    root = tomllib.loads(read("Cargo.toml"))
    members = root.get("workspace", {}).get("members", [])
    moved = [new for _, new, _ in MOVES if exists(new)]
    if not old_manifests and all(new in members for _, new, _ in MOVES if new != "crates/velnor-test-support"):
        state = "applied"
    elif len(old_manifests) == len(OLD_ROOTS) and not moved:
        state = "not applied"
    else:
        state = "partial"
    print(f"status: {state}")
    print(f"old workspace roots present: {old_manifests or 'none'}; moved crates present: {len(moved)}/{len(MOVES)}")
    hits = {}
    for path in tracked():
        if skipped(path) or is_engine(path) or not exists(path) or path.endswith((".rs", ".lock")):
            continue
        if any(path.startswith(new + "/") for _, new, _ in MOVES) and path not in TOKEN_FILES:
            continue
        try:
            text = read(path)
        except (UnicodeDecodeError, IsADirectoryError):
            continue
        found = [m.group(1) for m in TOKEN.finditer(text) if map_path(m.group(1)) != m.group(1)]
        if found and path != ".alint.yml":
            hits[path] = found
    coupled = {path: found for path, found in hits.items() if is_coupled(path)}
    hits = {path: found for path, found in hits.items() if path not in coupled}
    print(f"stranded files under old roots: {len(stranded_files()) + len(leftover_files())}")
    print(f"files outside the skip list with old-path tokens: {len(hits)}")
    print(f"engine-coupled files still on old paths (rewritten by apply --engine-path-neutral): {len(coupled)}")
    for path, found in sorted(hits.items())[:40]:
        print(f"  {path}: {len(found)} e.g. {found[0]}")
    return state


def prune_empty(directory):
    while directory:
        try:
            os.rmdir(os.path.join(ROOT, directory))
        except OSError:
            return
        directory = posixpath.dirname(directory)


def clean_tree():
    return not git("status", "--porcelain").strip()


def run_rewrites(mode, only=None):
    files, dirs = tree_state(tracked())
    changed = 0
    for path in sorted(files):
        full = os.path.join(ROOT, path)
        if skipped(path) or not os.path.isfile(full) or os.path.islink(full):
            continue
        if only is not None and path not in only:
            continue
        if path.endswith((".lock",)) or path in OLD_ROOT_FILES:
            continue
        try:
            text = read(path)
        except UnicodeDecodeError:
            continue
        new = transform(path, text, files, dirs, mode)
        if new != text:
            write(path, new)
            changed += 1
    print(f"rewrote {changed} files ({mode})")
    return changed


def rewrite_release_tools_workflow():
    if not exists(RELEASE_TOOLS_WORKFLOW):
        return
    text = read(RELEASE_TOOLS_WORKFLOW)
    for old, new in RELEASE_TOOLS_EDITS:
        if old in text:
            text = text.replace(old, new)
        elif new not in text:
            sys.exit(f"release-tools anchor missing in {RELEASE_TOOLS_WORKFLOW}: {old!r}")
    write(RELEASE_TOOLS_WORKFLOW, text)


def cmd_status(_args):
    status_report()


def cmd_apply(args):
    old_manifests = [root for root in OLD_ROOTS if exists(f"{root}/Cargo.toml")]
    if not old_manifests:
        print("already applied; running the fixup pass to prove a re-run changes nothing")
        run_rewrites("fixup")
        rewrite_release_tools_workflow()
        protected_note()
        return
    if len(old_manifests) != len(OLD_ROOTS):
        sys.exit("partial state: some old workspace roots are gone; reset the tree and run apply again")
    for old, new, _ in MOVES:
        if exists(old) and exists(new):
            sys.exit(f"destination {new} already exists; remove it and run apply again")
    if not args.allow_dirty and not clean_tree():
        sys.exit("working tree is not clean; commit or reset first")
    print("base:", git("rev-parse", "HEAD").strip())
    before_trees = collect_trees(True)
    old_texts = {root: read(f"{root}/Cargo.toml") for root in OLD_ROOTS}
    old_locks = [read("Cargo.lock")] + [read(f"{root}/Cargo.lock") for root in OLD_ROOTS]
    nextest = merged_nextest(read(".config/nextest.toml"), read("generator/.config/nextest.toml"), read("ci-helper/.config/nextest.toml"))
    deny = merged_deny(read("deny.toml"))
    root_manifest = build_root_manifest(read("Cargo.toml"), old_texts)
    run_rewrites("apply")
    rewrite_release_tools_workflow()
    for old, new, _ in MOVES:
        if exists(old):
            git("mv", old, new)
    for path in OLD_ROOT_FILES:
        if exists(path):
            git("rm", "-q", "-f", path)
    for old, _, _ in MOVES:
        prune_empty(posixpath.dirname(old))
    for path in OLD_ROOT_FILES:
        prune_empty(posixpath.dirname(path))
    write("Cargo.toml", root_manifest)
    write(".config/nextest.toml", nextest)
    write("deny.toml", deny)
    append_secret_baselines()
    merge_locks(old_locks)
    subprocess.run(["cargo", "fmt", "--all"], cwd=ROOT, check=True)
    git("add", "-A")
    report = version_report(before_trees, collect_trees(False))
    print(report)
    if args.lock_report:
        with open(args.lock_report, "w", encoding="utf-8") as handle:
            handle.write(report)
    protected_note()
    print("applied")


def changed_files(base):
    merge_base = git("merge-base", "HEAD", base).strip()
    changed = set(git("diff", "--name-only", "--no-renames", merge_base, "HEAD").split("\n"))
    changed |= set(git("diff", "--name-only", "--no-renames", "HEAD").split("\n"))
    return {path for path in changed if path}


def cmd_fixup(args):
    moved, blocked = move_stranded()
    if moved:
        print(f"moved {len(moved)} stranded files to their new paths")
    run_rewrites("fixup", only=changed_files(args.base) | set(moved))
    reports = deleted_file_edits(args.base)
    problems = []
    if blocked:
        problems.append("Stranded files that could not be moved:\n" + "\n".join(f"  {line}" for line in blocked))
    leftovers = leftover_files()
    if leftovers:
        problems.append(
            "Files under an old root that are not in the move table and not stay-put (move them by hand):\n"
            + "\n".join(f"  {path}" for path in leftovers)
        )
    problems += reports
    for line in UNRESOLVED:
        print(f"WARNING unresolved relative path, check by hand: {line}")
    if problems:
        sys.exit("\n\n".join(problems))
    print("branch-fixup: OK")


def main():
    global ROOT, ROOT_VERSION, ENGINE_NEUTRAL
    parser = argparse.ArgumentParser(description="Move every crate to crates/velnor-* under one root workspace")
    parser.add_argument("--root", default=None)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("status").set_defaults(run=cmd_status)
    apply = sub.add_parser("apply")
    apply.add_argument("--allow-dirty", action="store_true")
    apply.add_argument("--engine-path-neutral", action="store_true")
    apply.add_argument("--lock-report", default=None)
    apply.set_defaults(run=cmd_apply)
    fixup = sub.add_parser("branch-fixup")
    fixup.add_argument("--base", default="origin/main")
    fixup.set_defaults(run=cmd_fixup)
    args = parser.parse_args()
    ENGINE_NEUTRAL = getattr(args, "engine_path_neutral", False)
    ROOT = args.root or subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip()
    with open(os.path.join(ROOT, "Cargo.toml"), "rb") as handle:
        ROOT_VERSION = tomllib.load(handle).get("workspace", {}).get("package", {}).get("version")
    args.run(args)


if __name__ == "__main__":
    main()
