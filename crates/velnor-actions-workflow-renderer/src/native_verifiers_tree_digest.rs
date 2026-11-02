// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeDigestPin {
    version: &'static str,
    target: &'static str,
    components_sha256: &'static str,
    tree_sha256: &'static str,
}

pub const RUST_TREE_DIGESTS: &[TreeDigestPin] = &[];

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

const NOTICE: &str = r#"pm() { printf "%s\n" "::notice::pin_mismatch: $1"; }"#;

const TOOLCHAINS_SINGLE: &str = r#"set --; for e in "$rh/toolchains"/* "$rh/toolchains"/.[!.]* "$rh/toolchains"/..?*; do if [ -e "$e" ] || [ -L "$e" ]; then set -- "$@" "$e"; fi; done; [ "$#" -eq 1 ] || { pm toolchains_not_single; return 1; }"#;

const COMPONENTS_HASH: &str = r#"sort "$t/lib/rustlib/components" | $h > "$d/pin.cs"; read -r cs rest < "$d/pin.cs" || { pm components_unreadable; return 1; }"#;

const SCAN: &str = r#"(cd "$t" && find . -name "*[!A-Za-z0-9._+-]*") > "$d/pin.unsafe" || { pm tree_unreadable; return 1; }; [ ! -s "$d/pin.unsafe" ] || { pm unsafe_name; return 1; }; (cd "$t" && find . -type f -exec $h {} +) > "$d/pin.sums" || { pm tree_unreadable; return 1; }; (cd "$t" && find . -exec ls -ldn {} +) > "$d/pin.modes" || { pm tree_unreadable; return 1; }"#;

const LISTING_AWK: &str = r#"awk -v cs="$cs" -v cap=4194304 'function bits(r, w, x, v) { v = 0; if (r == "r") v += 4; if (w == "w") v += 2; if (x == "x" || x == "s" || x == "t") v += 1; return v } function sp(x, a) { return (x == "s" || x == "S" || x == "t" || x == "T") ? a : 0 } NR == FNR { dg[$2] = $1; next } { m = $1; k = substr(m, 1, 1); p = $NF; if (k != "-" && k != "d") exit 2; mode = sprintf("%d%d%d%d", sp(substr(m, 4, 1), 4) + sp(substr(m, 7, 1), 2) + sp(substr(m, 10, 1), 1), bits(substr(m, 2, 1), substr(m, 3, 1), substr(m, 4, 1)), bits(substr(m, 5, 1), substr(m, 6, 1), substr(m, 7, 1)), bits(substr(m, 8, 1), substr(m, 9, 1), substr(m, 10, 1))); r = (p == ".") ? "." : substr(p, 3); if (k == "d") { print "d", mode, "-", r; next } if (r ~ /^lib\/rustlib\/(manifest-[A-Za-z0-9._+-]+|multirust-channel-manifest\.toml|multirust-config\.toml|rust-installer-version)$/) { if ($5 + 0 > cap + 0) exit 3; print "b", mode, "-", r; next } if (r == "lib/rustlib/components") { print "f", mode, cs, r; next } if (!(p in dg)) exit 4; print "f", mode, dg[p], r }' "$d/pin.sums" "$d/pin.modes" > "$d/pin.list" || { pm listing_failed; return 1; }"#;

const COMPARE: &str = r#"sort -k4,4 "$d/pin.list" | tr "\n" "\000" | $h > "$d/pin.got"; read -r got rest < "$d/pin.got" || { pm tree_unreadable; return 1; }; [ "$got" = "$pin" ] || { pm tree_differs; return 1; }"#;

pub(crate) fn pin_statements(pins: &[TreeDigestPin]) -> Vec<String> {
    let rows = pins
        .iter()
        .filter(|pin| pin.is_well_formed())
        .map(TreeDigestPin::case_row)
        .collect::<String>();
    let select = format!(
        "pin=\"\"; case \"$v:$tgt:$cs\" in {rows}*) ;; esac; [ -n \"$pin\" ] || {{ pm no_pin; return 1; }}"
    );
    let body = [
        "LC_ALL=C; export LC_ALL",
        TOOLCHAINS_SINGLE,
        r#"tgt="${t##*/}"; tgt="${tgt#"$v"-}""#,
        COMPONENTS_HASH,
        &select,
        SCAN,
        LISTING_AWK,
        COMPARE,
    ]
    .join("; ");
    vec![
        NOTICE.to_owned(),
        format!("pin_check() ( {body} )"),
        "if pin_check; then printf \"%s\\n\" \"velnor: Rust $v toolchain matches pinned tree digest\"; exit 0; fi".to_owned(),
    ]
}
