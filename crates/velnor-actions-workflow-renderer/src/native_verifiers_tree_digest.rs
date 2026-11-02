// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
        "4e04ef08846d5545de6eae7492e7c71e330616da65313274d6a8a28769e5510e",
    ),
    TreeDigestPin::new(
        "1.99.1",
        "x86_64-unknown-linux-gnu",
        "a13769c18a7a74ea492ab633b644d6c238ae81e536d30d3f168eddb57e86000e",
        "a02e015075bdd2b5ea02cce7828c2bb806e1aabf5e26696e8bbad54f162b092e",
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
            "{}:{}:{}) pin={};; ",
            self.version, self.target, self.components_sha256, self.tree_sha256
        )
    }
}

pub(crate) const URLS_FUNCTION: &str = r#"urls() { awk '/^$/ { next } /^\[\[?[A-Za-z0-9._"*+-]*\]\]?$/ { sec = ""; if ($0 ~ /^\[pkg\.[^]]*\.target\.[^]]*\]$/) sec = $0; next } /^[a-z0-9_-]+ = ("[^"]*"|true|false|\[[^]]*\])$/ { if (sec != "" && $1 ~ /(^|_)(url|hash)$/) print sec, $1, substr($0, index($0, " = ") + 3); next } { bad = 1 } END { exit bad }' "$1" > "$2.raw" && LC_ALL=C sort "$2.raw" > "$2"; }"#;

const NOTICE: &str = r#"pm() { printf "%s\n" "::notice::pin_mismatch: $1"; }"#;

const TOOLCHAINS_SINGLE: &str = r#"set --; for e in "$rh/toolchains"/* "$rh/toolchains"/.[!.]* "$rh/toolchains"/..?*; do if [ -e "$e" ] || [ -L "$e" ]; then set -- "$@" "$e"; fi; done; [ "$#" -eq 1 ] || { pm toolchains_not_single; return 1; }"#;

const COMPONENTS_HASH: &str = r#"sort "$t/lib/rustlib/components" | $h > "$d/pin.cs"; read -r cs rest < "$d/pin.cs" || { pm components_unreadable; return 1; }"#;

const BOOKKEEPING: &str = r#"urls "$t/lib/rustlib/multirust-channel-manifest.toml" "$d/pin.urls" || { pm bookkeeping_manifest_unreadable; return 2; }; $h < "$d/pin.urls" > "$d/pin.ms"; read -r ms rest < "$d/pin.ms" || { pm bookkeeping_manifest_unreadable; return 2; }"#;

const SCAN: &str = r#"(cd "$t" && find . -name "*[!A-Za-z0-9._+-]*") > "$d/pin.unsafe" || { pm tree_unreadable; return 2; }; [ ! -s "$d/pin.unsafe" ] || { pm unsafe_name; return 2; }; (cd "$t" && find . -type f -exec $h {} +) > "$d/pin.sums" || { pm tree_unreadable; return 2; }; (cd "$t" && find . -exec ls -ldn {} +) > "$d/pin.modes" || { pm tree_unreadable; return 2; }"#;

const LISTING_AWK: &str = r#"awk -v cs="$cs" -v ms="$ms" -v cap=4194304 'function bits(r, w, x, v) { v = 0; if (r == "r") v += 4; if (w == "w") v += 2; if (x == "x" || x == "s" || x == "t") v += 1; return v } function sp(x, a) { return (x == "s" || x == "S" || x == "t" || x == "T") ? a : 0 } NR == FNR { dg[$2] = $1; next } { m = $1; k = substr(m, 1, 1); p = $NF; if (k != "-" && k != "d") exit 2; mode = sprintf("%d%d%d%d", sp(substr(m, 4, 1), 4) + sp(substr(m, 7, 1), 2) + sp(substr(m, 10, 1), 1), bits(substr(m, 2, 1), substr(m, 3, 1), substr(m, 4, 1)), bits(substr(m, 5, 1), substr(m, 6, 1), substr(m, 7, 1)), bits(substr(m, 8, 1), substr(m, 9, 1), substr(m, 10, 1))); r = (p == ".") ? "." : substr(p, 3); if (k == "d") { print "d", mode, "-", r; next } if (r == "lib/rustlib/multirust-config.toml") { if ($5 + 0 > cap + 0) exit 3; print "b", mode, "-", r; next } if (r == "lib/rustlib/multirust-channel-manifest.toml") { if ($5 + 0 > cap + 0) exit 3; print "f", mode, ms, r; next } if (r == "lib/rustlib/components") { print "f", mode, cs, r; next } if (!(p in dg)) exit 4; print "f", mode, dg[p], r }' "$d/pin.sums" "$d/pin.modes" > "$d/pin.list" || { pm listing_failed; return 2; }"#;

const COMPARE: &str = r#"sort -k4,4 "$d/pin.list" | tr "\n" "\000" | $h > "$d/pin.got"; read -r got rest < "$d/pin.got" || { pm tree_unreadable; return 2; }; [ "$got" = "$pin" ] || { pm tree_differs; return 2; }"#;

const DISPATCH: &str = r#"rc=0; pin_check || rc=$?; case "$rc" in 0) printf "%s\n" "velnor: Rust $v toolchain matches pinned tree digest"; exit 0;; 1) ;; *) fail "rust_tree_digest_mismatch: $t differs from the pinned tree digest of Rust $v; the pin_mismatch notice above names the check that failed" ;; esac"#;

pub(crate) fn pin_statements(pins: &[TreeDigestPin]) -> Vec<String> {
    let rows = pins
        .iter()
        .filter(|pin| pin.is_well_formed())
        .map(TreeDigestPin::case_row)
        .collect::<String>();
    let select = format!(
        "pin=\"\"; case \"$v:$tgt:$cs\" in {rows}*) ;; esac; [ -n \"$pin\" ] || {{ pm \"no_pin components=$cs\"; return 1; }}"
    );
    let body = [
        "LC_ALL=C; export LC_ALL",
        TOOLCHAINS_SINGLE,
        r#"tgt="${t##*/}"; tgt="${tgt#"$v"-}""#,
        COMPONENTS_HASH,
        &select,
        BOOKKEEPING,
        SCAN,
        LISTING_AWK,
        COMPARE,
    ]
    .join("; ");
    vec![
        NOTICE.to_owned(),
        format!("pin_check() ( {body} )"),
        DISPATCH.to_owned(),
    ]
}
