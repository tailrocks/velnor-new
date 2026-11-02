// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::native_verifiers::ToolchainPhase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeCheck<'a> {
    Pinned {
        target: &'a str,
        pins: &'a [TreeDigestPin],
    },
    FileByFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeDigestPin {
    version: &'static str,
    target: &'static str,
    components_sha256: &'static str,
    tree_sha256: &'static str,
}

pub const RUST_TREE_DIGESTS: &[TreeDigestPin] = &[
    TreeDigestPin::new(
        "1.99.1",
        "x86_64-unknown-linux-gnu",
        "e3808a4c78c316b41a42ed88010c6bbf24ae24a348a5a22b8e3bc62328db1981",
        "f9eeeb78948257bc3c861d018ecf4d22f16e38fbc8ba4a97c1126a1a1c45bb25",
    ),
    TreeDigestPin::new(
        "1.99.1",
        "x86_64-unknown-linux-gnu",
        "a13769c18a7a74ea492ab633b644d6c238ae81e536d30d3f168eddb57e86000e",
        "56452430d3faa33fd43083ef5dc0d3fb5da905f35515e6f174e46ecd3a607dea",
    ),
];

impl TreeDigestPin {
    #[must_use]
    pub const fn new(
        version: &'static str,
        target: &'static str,
        components_sha256: &'static str,
        tree_sha256: &'static str,
    ) -> Self {
        Self {
            version,
            target,
            components_sha256,
            tree_sha256,
        }
    }

    #[must_use]
    pub const fn version(&self) -> &'static str {
        self.version
    }

    #[must_use]
    pub const fn target(&self) -> &'static str {
        self.target
    }

    #[must_use]
    pub const fn components_sha256(&self) -> &'static str {
        self.components_sha256
    }

    #[must_use]
    pub const fn tree_sha256(&self) -> &'static str {
        self.tree_sha256
    }

    fn is_well_formed(&self) -> bool {
        let name = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        };
        let digest =
            |s: &str| s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        name(self.version)
            && name(self.target)
            && digest(self.components_sha256)
            && digest(self.tree_sha256)
    }

    fn case_row(&self) -> String {
        format!(
            "{}:{}) pin={};; ",
            self.version, self.components_sha256, self.tree_sha256
        )
    }
}

const FOREIGN_TARGET: &str = "aarch64-unknown-linux-gnu";

const NOTICE: &str = r#"pm() { printf "%s\n" "::notice::pin_mismatch: $1"; }"#;

const COMPONENTS_HASH: &str = r#"ev sort "$t/lib/rustlib/components" | $h > "$d/pin.cs"; read -r cs rest < "$d/pin.cs" || cs=unreadable"#;

const NO_PIN: &str = r#"[ "$pinned" = 1 ] || { pm "no_pin components=$cs"; return 1; }"#;

const SCAN: &str = r#"(cd "$t" && ev find . -name "*[!A-Za-z0-9._+-]*") > "$d/pin.unsafe" || { pm tree_unreadable; return 2; }; [ ! -s "$d/pin.unsafe" ] || { pm unsafe_name; return 2; }; (cd "$t" && ev find . -type f -exec $hb {} +) > "$d/pin.sums" || { pm tree_unreadable; return 2; }; (cd "$t" && ev find . -printf "%y %#m %s %p\n") > "$d/pin.modes" || { pm tree_unreadable; return 2; }"#;

const LISTING_AWK: &str = r#"ev awk -v cs="$cs" -v cap=4194304 'NR == FNR { dg[$2] = $1; next } { k = $1; mode = $2; p = $4; if (k != "f" && k != "d") exit 2; r = (p == ".") ? "." : substr(p, 3); if (k == "d") { print "d", mode, "-", r; next } if (r == "lib/rustlib/multirust-config.toml") { if ($3 + 0 > cap + 0) exit 3; print "b", mode, "-", r; next } if (r == "lib/rustlib/components") { print "f", mode, cs, r; next } if (!(p in dg)) exit 4; print "f", mode, dg[p], r }' "$d/pin.sums" "$d/pin.modes" > "$d/pin.list" || { pm listing_failed; return 2; }"#;

const COMPARE: &str = r#"ev sort -k4,4 "$d/pin.list" | ev tr "\n" "\000" | $h > "$d/pin.got"; read -r got rest < "$d/pin.got" || { pm tree_unreadable; return 2; }; [ "$got" = "$pin" ] || { pm tree_differs; return 2; }"#;

const STASH_VERIFIED_MANIFEST: &str = r#"put "$m" "$r/channel-rust-$v.toml" || :; "#;

fn dispatch(phase: ToolchainPhase) -> String {
    let stash = if phase == ToolchainPhase::Restored {
        STASH_VERIFIED_MANIFEST
    } else {
        ""
    };
    format!(
        r#"rc=0; pin_check || rc=$?; case "$rc" in 0) {stash}printf "%s\n" "velnor: Rust $v toolchain matches pinned tree digest"; exit 0;; 1) ;; *) fail "rust_tree_digest_mismatch: $t differs from the pinned tree digest of Rust $v; the pin_mismatch notice above names the check that failed" ;; esac"#
    )
}

fn rows<'a>(target: &'a str, pins: &'a [TreeDigestPin]) -> impl Iterator<Item = &'a TreeDigestPin> {
    pins.iter()
        .filter(move |pin| pin.target == target && pin.is_well_formed())
}

pub(crate) fn pin_scope_statements(check: TreeCheck<'_>) -> Vec<String> {
    let TreeCheck::Pinned { target, pins } = check else {
        return Vec::new();
    };
    let versions = rows(target, pins)
        .map(TreeDigestPin::version)
        .collect::<BTreeSet<_>>();
    if versions.is_empty() {
        return vec![NOTICE.to_owned(), "pinned=0".to_owned()];
    }
    let versions = versions.into_iter().collect::<Vec<_>>().join("|");
    let strict = format!(
        r#"[ "$#" -eq 0 ] || {{ [ "$#" -eq 1 ] && [ "$1" = "$rh/toolchains/$v-{target}" ]; }} || {{ pm toolchains_differ; fail "rust_tree_digest_mismatch: $rh/toolchains holds an entry other than the pinned Rust $v-{target} toolchain; the pin_mismatch notice above names the check that failed"; }}"#
    );
    let entries = if target == FOREIGN_TARGET {
        strict
    } else {
        format!(
            r#"if [ "$#" -eq 1 ] && [ "$1" = "$rh/toolchains/$v-{FOREIGN_TARGET}" ]; then pinned=0; else {strict}; fi"#
        )
    };
    vec![
        NOTICE.to_owned(),
        format!("pinned=0; case \"$v\" in {versions}) pinned=1;; esac"),
        format!(
            r#"[ "$pinned" = 0 ] || {{ set --; for e in "$rh/toolchains"/* "$rh/toolchains"/.[!.]* "$rh/toolchains"/..?*; do if [ -e "$e" ] || [ -L "$e" ]; then set -- "$@" "$e"; fi; done; {entries}; }}"#
        ),
    ]
}

pub(crate) fn pin_check_statements(check: TreeCheck<'_>, phase: ToolchainPhase) -> Vec<String> {
    let TreeCheck::Pinned { target, pins } = check else {
        return Vec::new();
    };
    let rows = rows(target, pins)
        .map(TreeDigestPin::case_row)
        .collect::<String>();
    let select = format!(
        "pin=\"\"; case \"$v:$cs\" in {rows}*) ;; esac; [ -n \"$pin\" ] || {{ pm \"components_not_pinned components=$cs\"; return 2; }}"
    );
    let body = [COMPONENTS_HASH, NO_PIN, &select, SCAN, LISTING_AWK, COMPARE].join("; ");
    vec![format!("pin_check() ( {body} )"), dispatch(phase)]
}
