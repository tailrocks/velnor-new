#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
checker="$repo/scripts/check-no-doctests.sh"
work="$(mktemp -d "${TMPDIR:-/tmp}/no-doctests-listing.XXXXXX")"
trap 'rm -rf "$work"' EXIT

fails=0
fence='```'
# check <name> <fail|pass> <lib.rs content with printf %b escapes>
check() {
  local name="$1" expect="$2" dir="$work/$1"
  mkdir -p "$dir/fx/src"
  printf '[workspace]\nmembers = ["fx"]\nresolver = "3"\n' > "$dir/Cargo.toml"
  printf '[package]\nname = "fx"\nversion = "0.0.0"\nedition = "2024"\n\n[lib]\ndoctest = false\n\n[features]\ngated = []\n' > "$dir/fx/Cargo.toml"
  printf '%b' "$3" > "$dir/fx/src/lib.rs"
  printf '%s\nlet x = 1;\n%s\n' "$fence" "$fence" > "$dir/fx/src/fence.md"
  if out="$(bash "$checker" "$dir" 2>&1)"; then
    got=pass
  elif printf '%s\n' "$out" | grep -q 'rustdoc lists the doctests above'; then
    got=fail
  else
    got=broken
  fi
  if [ "$got" != "$expect" ]; then
    echo "FAIL $name: expected $expect, got $got"
    fails=$((fails + 1))
  else
    echo "ok   $name ($got)"
  fi
}

check fenced_outer      fail '/// ```\n/// let x = 1;\n/// ```\nfn f() {}\n'
check fenced_inner_tilde fail '//! ~~~\n//! let x = 1;\n//! ~~~\n'
check fenced_ignore     fail '/// ```ignore\n/// let x = 1;\n/// ```\nfn f() {}\n'
check fenced_compile_fail fail '/// ```compile_fail\n/// let x: u8 = 256;\n/// ```\nfn f() {}\n'
check blank_then_indent fail '/// text\n///\n///     code\nfn f() {}\n'
check attr_doc_fence    fail '#[doc = "```\\nlet x = 1;\\n```"]\nfn f() {}\n'
check block_outer_fence fail '/** ```\n * let x = 1;\n * ```\n */\nfn f() {}\n'
check concat_split_fence fail '#[doc = concat!("``", "`\\nlet x = 1;\\n``", "`")]\nfn f() {}\n'
check include_str_fence fail '#[doc = include_str!("fence.md")]\nfn f() {}\n'
check cfg_attr_fence    fail '#![cfg_attr(all(), doc = "```\\nlet x = 1;\\n```")]\nfn f() {}\n'
check list5_bare        fail '/// -     let x = 1;\nfn f() {}\n'
check feature_gated_fence fail '#[cfg(feature = "gated")]\n/// ```\n/// let x = 1;\n/// ```\nfn f() {}\n'
check plain_prose       pass '//! inner prose\n/// plain prose\nfn f() {}\n'
check text_fence        pass '/// ```text\n/// not rust\n/// ```\nfn f() {}\n'
check prose_cont_line   pass '/// prose\n///     let x = 1;\nfn f() {}\n'
check plain_block_fence pass '/* ```\n * not a doc\n */\nfn f() {}\n'
check string_fence      pass 'fn f() { let s = "```"; let _ = s; }\n'

exit "$fails"
