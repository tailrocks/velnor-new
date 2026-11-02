// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::{JACTIONLINT_VERIFIER_USES, RUST_VERIFIER_USES};
use velnor_actions_contract::{Step, StepKind, StepRole};
use velnor_actions_mise::catalog::RUST_CHANNEL_MANIFESTS;
use velnor_actions_mise::{PinnedTool, RUST_TARGET_TRIPLE, ToolCatalog, ToolHomes};

use crate::VERIFY_PATH;
use crate::native_verifiers_tree_digest::{
    RUST_TREE_DIGESTS, TreeCheck, pin_check_statements, pin_scope_statements,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolchainPhase {
    Restored,
    Installed,
}

pub const NOT_CATALOGUED_CODE: &str = "rust_channel_manifest_not_catalogued";

pub const VERIFY_RESTORED_RUST_STEP: &str = "Verify restored Rust toolchain";
pub const VERIFY_JACTIONLINT_STEP: &str = "Verify jactionlint";

const DIST: &str = "https://static.rust-lang.org/dist/channel-rust-";

const OWNED_HOME_TREES: TreeCheck<'static> = TreeCheck::Pinned {
    target: RUST_TARGET_TRIPLE,
    pins: RUST_TREE_DIGESTS,
};

const HELPER_ENV: [&str; 18] = [
    "PATH",
    "HOME",
    "RUNNER_TEMP",
    "RUSTUP_HOME",
    "MISE_RUSTUP_HOME",
    "CARGO_HOME",
    "VELNOR_TOOL_RUST",
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "NO_PROXY",
    "no_proxy",
    "ALL_PROXY",
    "all_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CURL_CA_BUNDLE",
];

fn helper_runner() -> String {
    let allowed = HELPER_ENV
        .iter()
        .map(|name| format!("${{{name}+{name}=\"${name}\"}}"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("ev() {{ /usr/bin/env -i LC_ALL=C {allowed} \"$@\"; }}")
}

const COMPONENT_TARBALLS_AWK: &str = r#"ev awk -v comps="$t/lib/rustlib/components" 'BEGIN { while ((getline line < comps) > 0) if (line != "") want[line] = 1 } /^\[/ { key = ""; if ($0 ~ /^\[pkg\.[^]]*\.target\.[^]]*\]$/) { s = substr($0, 6, length($0) - 6); p = index(s, ".target."); k = substr(s, 1, p - 1) "-" substr(s, p + 8); if (k in want) key = k } next } key != "" && $1 == "xz_url" { u = $3; gsub(/"/, "", u); url[key] = u } key != "" && $1 == "xz_hash" { h = $3; gsub(/"/, "", h); hash[key] = h } END { for (k in want) { if (!(k in url) || !(k in hash)) exit 1; print k, url[k], hash[k] } }' "$d/channel.toml" > "$d/tarballs""#;

const GET: &str = "get() { ev curl -q --proto \"=https\" --proto-redir \"=https\" --tlsv1.2 --fail --silent --show-error --location --retry 3 --retry-all-errors --connect-timeout 20 --max-time 120 --retry-max-time 180 --output \"$1\" \"$2\"; }";

const PUT: &str = "put() { [ ! -d \"$2\" ] && ev rm -f \"$2.new\" && ev cp \"$1\" \"$2.new\" && ev chmod 0644 \"$2.new\" && ev mv -f \"$2.new\" \"$2\" || { ev rm -f \"$2.new\"; return 1; }; }";

const CHANNEL_SHA_OK: &str = "channel_ok() { printf \"%s  %s\\n\" \"$want\" \"$d/channel.toml\" > \"$d/channel.sha256\"; $h -c \"$d/channel.sha256\" >/dev/null 2>&1; }";

const VERIFIED_CHANNEL: &str = "verified_channel() { s=\"$r/channel-rust-$v.toml\"; if [ -f \"$s\" ] && [ ! -L \"$s\" ] && ev cp \"$s\" \"$d/channel.toml\" && channel_ok; then return 0; fi; get \"$d/channel.toml\" \"{DIST}$v.toml\"; channel_ok || fail \"rust_channel_manifest_digest_mismatch: channel-rust-$v.toml is not SHA-256 $want\"; put \"$d/channel.toml\" \"$s\" || :; }";

const INSTALL_MANIFEST: &str = "install_manifest() { [ ! -d \"$m\" ] || fail \"rust_toolchain_file_unexpected: $m is a directory\"; verified_channel; put \"$d/channel.toml\" \"$m\" || fail \"rust_installed_manifest_unwritable: $m\"; }";

#[must_use]
pub fn toolchain_script(
    version: &str,
    phase: ToolchainPhase,
    manifests: &[(&str, &str, &str)],
    trees: TreeCheck<'_>,
) -> String {
    let cases = manifests
        .iter()
        .map(|(known, digest, _)| [*known, ") want=", *digest, ";; "].concat())
        .collect::<String>();
    let mut lines = vec![
        crate::toolchain_env::credential_unset_prelude()
            .trim_end_matches(';')
            .to_owned(),
        "set -eu".to_owned(),
        format!("PATH={VERIFY_PATH}"),
        "export PATH".to_owned(),
        helper_runner(),
        format!("v=\"{version}\""),
        "fail() { printf \"%s\\n\" \"::error::$1\" >&2; exit 1; }".to_owned(),
        format!(
            "case \"$v\" in {cases}*) fail \"{NOT_CATALOGUED_CODE}: Rust $v has no channel-manifest digest in this Velnor catalog; upgrade velnor-actions or pin a catalogued Rust\";; esac"
        ),
        "rh=\"${RUSTUP_HOME:-${MISE_RUSTUP_HOME:-$HOME/.rustup}}\"".to_owned(),
        "r=\"$RUNNER_TEMP/velnor/rust-verified\"".to_owned(),
        "d=\"$RUNNER_TEMP/velnor/rust-manifest\"".to_owned(),
        "ev rm -rf \"$d\"".to_owned(),
    ];
    if phase == ToolchainPhase::Restored {
        lines.push("ev rm -rf \"$r\"".to_owned());
    }
    lines.push("ev mkdir -p \"$d\" \"$r\"".to_owned());
    if phase == ToolchainPhase::Restored {
        lines.push("ev rm -rf \"${CARGO_HOME:?}/bin\"".to_owned());
    }
    lines.extend(pin_scope_statements(trees));
    lines.push("set -- \"$rh/toolchains/$v\"-*".to_owned());
    if phase == ToolchainPhase::Restored {
        lines.push(
            "if [ \"$#\" -eq 1 ] && [ ! -e \"$1\" ] && [ ! -L \"$1\" ]; then printf \"%s\\n\" \"velnor: no restored Rust $v toolchain; the install verifies the fresh one\"; exit 0; fi"
                .to_owned(),
        );
    }
    lines.extend([
        "{ [ \"$#\" -eq 1 ] && [ -d \"$1\" ] && [ ! -L \"$1\" ]; } || fail \"rust_toolchain_missing: $rh/toolchains holds no single real Rust $v toolchain directory\"".to_owned(),
        "t=\"$1\"".to_owned(),
        "m=\"$t/lib/rustlib/multirust-channel-manifest.toml\"".to_owned(),
        "if command -v sha256sum >/dev/null 2>&1; then hb=\"sha256sum\"; else hb=\"shasum -a 256\"; fi".to_owned(),
        "h=\"ev $hb\"".to_owned(),
        "test -f \"$t/lib/rustlib/components\" || fail \"rust_installed_manifest_missing: $t/lib/rustlib/components\"".to_owned(),
        "ev find \"$t\" ! -type f ! -type d > \"$d/special\"".to_owned(),
        "test ! -s \"$d/special\" || fail \"rust_toolchain_file_unexpected: $t holds a symlink or a special file\"".to_owned(),
        GET.to_owned(),
        PUT.to_owned(),
        CHANNEL_SHA_OK.to_owned(),
        VERIFIED_CHANNEL.replace("{DIST}", DIST),
        INSTALL_MANIFEST.to_owned(),
    ]);
    if phase == ToolchainPhase::Installed {
        lines.push("install_manifest".to_owned());
    }
    lines.extend(pin_check_statements(trees, phase));
    if phase == ToolchainPhase::Restored {
        lines.push("install_manifest".to_owned());
    }
    lines.extend([
        format!("{COMPONENT_TARBALLS_AWK} || fail \"rust_component_not_in_manifest: $t lists a component the verified channel-rust-$v.toml does not ship\""),
        "test -s \"$d/tarballs\" || fail \"rust_installed_manifest_empty: $t lists no component\"".to_owned(),
        ": > \"$d/expected\"".to_owned(),
        "while read -r comp url hash; do case \"$comp\" in *[!A-Za-z0-9._-]*|\"\") fail \"rust_component_name_invalid: $comp\";; esac; case \"$url\" in https://static.rust-lang.org/dist/*.tar.xz) ;; *) fail \"rust_component_url_untrusted: $url\";; esac; if [ ! -f \"$r/$comp.$hash\" ]; then get \"$d/c.tar.xz\" \"$url\"; printf \"%s  %s\\n\" \"$hash\" \"$d/c.tar.xz\" > \"$d/c.sha256\"; $h -c \"$d/c.sha256\" >/dev/null || fail \"rust_component_tarball_digest_mismatch: $comp\"; ev rm -rf \"$d/x\"; ev mkdir \"$d/x\"; ev tar -xJf \"$d/c.tar.xz\" -C \"$d/x\" --strip-components=1; ev rm -f \"$d/c.tar.xz\"; : > \"$d/c.sums\"; while read -r sub; do case \"$sub\" in *[!A-Za-z0-9._-]*|\"\"|.|..) fail \"rust_component_name_invalid: $sub\";; esac; (cd \"$d/x/$sub\" && ev find . -type f ! -path ./manifest.in -exec $hb {} +) >> \"$d/c.sums\"; done < \"$d/x/components\"; ev mv \"$d/c.sums\" \"$r/$comp.$hash\"; fi; ev cat \"$r/$comp.$hash\" >> \"$d/expected\"; done < \"$d/tarballs\"".to_owned(),
        "(cd \"$t\" && ev find . -type f ! -path ./lib/rustlib/components ! \\( -path \"./lib/rustlib/manifest-*\" ! -path \"./lib/rustlib/manifest-*/*\" \\) ! -path ./lib/rustlib/multirust-channel-manifest.toml ! -path ./lib/rustlib/multirust-config.toml ! -path ./lib/rustlib/rust-installer-version -exec $hb {} +) > \"$d/installed\"".to_owned(),
        "ev sort -k2 \"$d/expected\" > \"$d/expected.sorted\"".to_owned(),
        "ev sort -k2 \"$d/installed\" > \"$d/installed.sorted\"".to_owned(),
        "if ! ev cmp -s \"$d/expected.sorted\" \"$d/installed.sorted\"; then ev diff \"$d/expected.sorted\" \"$d/installed.sorted\" | ev head -n 20 >&2 || true; fail \"rust_toolchain_bytes_mismatch: $t differs from the component tarballs of the verified channel-rust-$v.toml\"; fi".to_owned(),
        "printf \"%s\\n\" \"velnor: Rust $v toolchain verified file by file against the component tarballs of channel-rust-$v.toml (SHA-256 $want)\"".to_owned(),
    ]);
    lines.join("; ")
}

pub fn rust_verifier_shell_step(
    catalog: &ToolCatalog,
    phase: ToolchainPhase,
) -> Result<Step, crate::RenderError> {
    let name = match phase {
        ToolchainPhase::Restored => VERIFY_RESTORED_RUST_STEP,
        ToolchainPhase::Installed => crate::VERIFY_RUST_TOOLCHAIN_STEP,
    };
    let homes = ToolHomes::runner_temp();
    let env = BTreeMap::from([
        ("RUSTUP_HOME".to_owned(), homes.rustup_home().to_owned()),
        ("CARGO_HOME".to_owned(), homes.cargo_home().to_owned()),
    ]);
    let script = toolchain_script(
        catalog.version(PinnedTool::Rust),
        phase,
        RUST_CHANNEL_MANIFESTS,
        OWNED_HOME_TREES,
    );
    crate::verifier_composite::verifier_shell_step(
        StepRole::VerifyRustManifest,
        name,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )
}

#[must_use]
pub fn verify_rust_owned_argv(catalog: &ToolCatalog, phase: ToolchainPhase) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        toolchain_script(
            catalog.version(PinnedTool::Rust),
            phase,
            RUST_CHANNEL_MANIFESTS,
            OWNED_HOME_TREES,
        ),
    ]
}

#[must_use]
pub fn verify_rust_argv(catalog: &ToolCatalog) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        toolchain_script(
            catalog.version(PinnedTool::Rust),
            ToolchainPhase::Installed,
            RUST_CHANNEL_MANIFESTS,
            TreeCheck::FileByFile,
        ),
    ]
}

#[must_use]
pub fn verify_rust_manifest_step() -> Step {
    call_step(
        crate::VERIFY_RUST_TOOLCHAIN_STEP,
        RUST_VERIFIER_USES[1],
        Some(StepRole::VerifyRustManifest),
    )
}

#[must_use]
pub fn verify_restored_rust_step() -> Step {
    call_step(
        VERIFY_RESTORED_RUST_STEP,
        RUST_VERIFIER_USES[0],
        Some(StepRole::VerifyRustManifest),
    )
}

#[must_use]
pub fn jactionlint_verifier_script(catalog: &ToolCatalog) -> String {
    crate::jactionlint_verifier_script(catalog.version(PinnedTool::Jactionlint))
}

#[must_use]
pub fn jactionlint_verifier_argv(catalog: &ToolCatalog) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        jactionlint_verifier_script(catalog),
    ]
}

pub fn jactionlint_verifier_shell_step(catalog: &ToolCatalog) -> Result<Step, crate::RenderError> {
    crate::steps::jactionlint_verifier_step(
        VERIFY_JACTIONLINT_STEP,
        jactionlint_verifier_argv(catalog),
        BTreeMap::new(),
    )
}

#[must_use]
pub fn verify_jactionlint_step() -> Step {
    call_step(
        VERIFY_JACTIONLINT_STEP,
        JACTIONLINT_VERIFIER_USES,
        Some(StepRole::VerifyJactionlint),
    )
}

fn call_step(name: &str, uses: &'static str, role: Option<StepRole>) -> Step {
    Step {
        name: name.to_owned(),
        id: None,
        role,
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}
