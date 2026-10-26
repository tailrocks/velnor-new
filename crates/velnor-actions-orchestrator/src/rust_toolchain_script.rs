// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_workflow_renderer::VERIFY_PATH;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolchainPhase {
    Restored,
    Installed,
}

pub(crate) const NOT_CATALOGUED_CODE: &str = "rust_channel_manifest_not_catalogued";

const DIST: &str = "https://static.rust-lang.org/dist/channel-rust-";

const COMPONENT_TARBALLS_AWK: &str = r#"awk -v comps="$t/lib/rustlib/components" 'BEGIN { while ((getline line < comps) > 0) if (line != "") want[line] = 1 } /^\[/ { key = ""; if ($0 ~ /^\[pkg\.[^]]*\.target\.[^]]*\]$/) { s = substr($0, 6, length($0) - 6); p = index(s, ".target."); k = substr(s, 1, p - 1) "-" substr(s, p + 8); if (k in want) key = k } next } key != "" && $1 == "xz_url" { u = $3; gsub(/"/, "", u); url[key] = u } key != "" && $1 == "xz_hash" { h = $3; gsub(/"/, "", h); hash[key] = h } END { for (k in want) { if (!(k in url) || !(k in hash)) exit 1; print k, url[k], hash[k] } }' "$d/channel.toml" > "$d/tarballs""#;

pub(crate) fn toolchain_script(
    version: &str,
    phase: ToolchainPhase,
    manifests: &[(&str, &str, &str)],
) -> String {
    let cases = manifests
        .iter()
        .map(|(known, digest, _)| [*known, ") want=", *digest, ";; "].concat())
        .collect::<String>();
    let mut lines = vec![
        velnor_actions_workflow_renderer::toolchain_env::credential_unset_prelude()
            .trim_end_matches(';')
            .to_owned(),
        "set -eu".to_owned(),
        format!("PATH={VERIFY_PATH}"),
        "export PATH".to_owned(),
        format!("v=\"{version}\""),
        "fail() { printf \"%s\\n\" \"::error::$1\" >&2; exit 1; }".to_owned(),
        format!(
            "case \"$v\" in {cases}*) fail \"{NOT_CATALOGUED_CODE}: Rust $v has no channel-manifest digest in this Velnor catalog; upgrade velnor-actions or pin a catalogued Rust\";; esac"
        ),
        "rh=\"${RUSTUP_HOME:-${MISE_RUSTUP_HOME:-$HOME/.rustup}}\"".to_owned(),
        "store=\"$rh/velnor-verified\"".to_owned(),
        "d=\"$RUNNER_TEMP/velnor/rust-manifest\"".to_owned(),
        "[ ! -L \"$store\" ] || fail \"rust_verified_store_unexpected: $store is a symlink\""
            .to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        ": > \"$d/ofetches.log\"".to_owned(),
        ": > \"$d/obytes.log\"".to_owned(),
        ": > \"$d/reused.log\"".to_owned(),
        ": > \"$d/vcomps.log\"".to_owned(),
    ];
    if phase == ToolchainPhase::Restored {
        lines.push("rm -rf \"${CARGO_HOME:?}/bin\"".to_owned());
    }
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
        "if command -v sha256sum >/dev/null 2>&1; then h=\"sha256sum\"; else h=\"shasum -a 256\"; fi".to_owned(),
        "get() { curl --proto \"=https\" --proto-redir \"=https\" --tlsv1.2 --fail --silent --show-error --location --retry 3 --output \"$1\" \"$2\"; }".to_owned(),
        "mstored=\"$store/channel-$v-$want.toml\"".to_owned(),
        "mreused=".to_owned(),
        "if [ -f \"$mstored\" ] && [ ! -L \"$mstored\" ]; then printf \"%s  %s\\n\" \"$want\" \"$mstored\" > \"$d/channel.sha256\"; if $h -c \"$d/channel.sha256\" >/dev/null; then mreused=1; echo reuse >> \"$d/reused.log\"; cp \"$mstored\" \"$d/channel.toml\"; else rm -f \"$mstored\"; fi; fi".to_owned(),
        format!("if [ -z \"$mreused\" ]; then get \"$d/channel.toml\" \"{DIST}$v.toml\"; printf \"%s  %s\\n\" \"$want\" \"$d/channel.toml\" > \"$d/channel.sha256\"; $h -c \"$d/channel.sha256\" >/dev/null || fail \"rust_channel_manifest_digest_mismatch: channel-rust-$v.toml is not SHA-256 $want\"; echo fetch >> \"$d/ofetches.log\"; wc -c < \"$d/channel.toml\" >> \"$d/obytes.log\"; mkdir -p \"$store\"; cp \"$d/channel.toml\" \"$mstored\"; fi"),
        "test -f \"$t/lib/rustlib/components\" || fail \"rust_installed_manifest_missing: $t/lib/rustlib/components\"".to_owned(),
        "find \"$t\" ! -type f ! -type d > \"$d/special\"".to_owned(),
        "test ! -s \"$d/special\" || fail \"rust_toolchain_file_unexpected: $t holds a symlink or a special file\"".to_owned(),
        format!("{COMPONENT_TARBALLS_AWK} || fail \"rust_component_not_in_manifest: $t lists a component the verified channel-rust-$v.toml does not ship\""),
        "test -s \"$d/tarballs\" || fail \"rust_installed_manifest_empty: $t lists no component\"".to_owned(),
        ": > \"$d/expected\"".to_owned(),
        "while read -r comp url hash; do case \"$comp\" in *[!A-Za-z0-9._-]*|\"\") fail \"rust_component_name_invalid: $comp\";; esac; case \"$url\" in https://static.rust-lang.org/dist/*.tar.xz) ;; *) fail \"rust_component_url_untrusted: $url\";; esac; stored=\"$store/$comp.$hash.tar.xz\"; creused=; if [ -f \"$stored\" ] && [ ! -L \"$stored\" ]; then printf \"%s  %s\\n\" \"$hash\" \"$stored\" > \"$d/c.sha256\"; if $h -c \"$d/c.sha256\" >/dev/null; then creused=1; echo reuse >> \"$d/reused.log\"; cp \"$stored\" \"$d/c.tar.xz\"; else rm -f \"$stored\"; fi; fi; if [ -z \"$creused\" ]; then get \"$d/c.tar.xz\" \"$url\"; printf \"%s  %s\\n\" \"$hash\" \"$d/c.tar.xz\" > \"$d/c.sha256\"; $h -c \"$d/c.sha256\" >/dev/null || fail \"rust_component_tarball_digest_mismatch: $comp\"; echo fetch >> \"$d/ofetches.log\"; wc -c < \"$d/c.tar.xz\" >> \"$d/obytes.log\"; mkdir -p \"$store\"; cp \"$d/c.tar.xz\" \"$stored\"; fi; rm -rf \"$d/x\"; mkdir \"$d/x\"; tar -xJf \"$d/c.tar.xz\" -C \"$d/x\" --strip-components=1; rm -f \"$d/c.tar.xz\"; : > \"$d/c.sums\"; while read -r sub; do case \"$sub\" in *[!A-Za-z0-9._-]*|\"\"|.|..) fail \"rust_component_name_invalid: $sub\";; esac; (cd \"$d/x/$sub\" && find . -type f ! -path ./manifest.in -exec $h {} +) >> \"$d/c.sums\"; done < \"$d/x/components\"; cat \"$d/c.sums\" >> \"$d/expected\"; echo component >> \"$d/vcomps.log\"; done < \"$d/tarballs\"".to_owned(),
        "(cd \"$t\" && find . -type f ! -path ./lib/rustlib/components ! -path \"./lib/rustlib/manifest-*\" ! -path ./lib/rustlib/multirust-channel-manifest.toml ! -path ./lib/rustlib/multirust-config.toml ! -path ./lib/rustlib/rust-installer-version -exec $h {} +) > \"$d/installed\"".to_owned(),
        "LC_ALL=C sort -k2 \"$d/expected\" > \"$d/expected.sorted\"".to_owned(),
        "LC_ALL=C sort -k2 \"$d/installed\" > \"$d/installed.sorted\"".to_owned(),
        "if ! cmp -s \"$d/expected.sorted\" \"$d/installed.sorted\"; then LC_ALL=C diff \"$d/expected.sorted\" \"$d/installed.sorted\" | head -n 20 >&2 || true; fail \"rust_toolchain_bytes_mismatch: $t differs from the component tarballs of the verified channel-rust-$v.toml\"; fi".to_owned(),
        "printf \"%s\\n\" \"velnor: Rust $v toolchain verified file by file against the component tarballs of channel-rust-$v.toml (SHA-256 $want)\"".to_owned(),
        "wc -l < \"$d/ofetches.log\" > \"$d/n.ofetches\"; read -r ofetches _ < \"$d/n.ofetches\"".to_owned(),
        "wc -l < \"$d/reused.log\" > \"$d/n.reused\"; read -r reused_archives _ < \"$d/n.reused\"".to_owned(),
        "wc -l < \"$d/vcomps.log\" > \"$d/n.vcomps\"; read -r vcomps _ < \"$d/n.vcomps\"".to_owned(),
        "awk '{ if ($1 ~ /^[0-9]+$/) s+=$1; else bad=1 } END { if (bad) print \"unknown\"; else print s+0 }' \"$d/obytes.log\" > \"$d/n.obytes\"; read -r obytes _ < \"$d/n.obytes\"; case \"$obytes\" in unknown) obytes_unknown=1;; esac".to_owned(),
        "report=\"velnor rust verify: version $v origin-fetches=$ofetches reused-archives=$reused_archives verified-components=$vcomps\"".to_owned(),
        "[ -n \"$obytes_unknown\" ] || report=\"$report origin-bytes=$obytes\"".to_owned(),
        "printf \"%s\\n\" \"$report\"".to_owned(),
        "if [ -n \"${GITHUB_STEP_SUMMARY-}\" ]; then printf \"%s\\n\" \"$report\" >> \"$GITHUB_STEP_SUMMARY\"; fi".to_owned(),
    ]);
    lines.join("; ")
}
