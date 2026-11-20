#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
config="${1:-$repo/.alint.yml}"
alint_bin="${ALINT:-alint}"
work="$(mktemp -d "${TMPDIR:-/tmp}/no-doctests.XXXXXX")"
trap 'rm -rf "$work"' EXIT

rule="$work/rule.yml"
{
  printf 'version: 1\nrules:\n'
  awk '/^  - id: no-doctests$/ {on = 1; print; next} on && /^  [^ ]/ {exit} on {print}' "$config"
} > "$rule"
grep -q 'id: no-doctests' "$rule" || { echo "no-doctests rule not found in $config" >&2; exit 2; }

fails=0
# check <name> <fail|pass> <file content with printf %b escapes>
check() {
  local name="$1" expect="$2" dir="$work/$1"
  mkdir -p "$dir/crates/fx/src"
  printf '%b' "$3" > "$dir/crates/fx/src/lib.rs"
  if "$alint_bin" check -c "$rule" "$dir" >/dev/null 2>&1; then got=pass; else got=fail; fi
  if [ "$got" != "$expect" ]; then
    echo "FAIL $name: expected $expect, got $got"
    fails=$((fails + 1))
  else
    echo "ok   $name ($got)"
  fi
}

check fenced_outer      fail '/// ```\n/// let x = 1;\n/// ```\nfn f() {}\n'
check fenced_inner      fail '//! ~~~\n//! let x = 1;\n//! ~~~\n'
check blank_then_indent fail '/// text\n///\n///     code\nfn f() {}\n'
check heading_indent    fail '/// # Title\n///     code\nfn f() {}\n'
check attr_between      fail '/// prose\n///\n#[allow(dead_code)]\n///     code\nfn f() {}\n'
check crlf_blank_indent fail '/// a\r\n///\r\n///     code\r\nfn f() {}\r\n'
check include_str       fail '#[doc = include_str!("README.md")]\nfn f() {}\n'
check attr_doc_fence    fail '#[doc = "```\\nlet x = 1;\\n```"]\nfn f() {}\n'
check attr_nested_include fail '#![cfg_attr(all(), doc = include_str!("README.md"))]\nfn f() {}\n'
check indent_no_blank   fail '/// - item\n///     onto the next line\nfn f() {}\n'
check block_outer_fence fail '/** ```\n * let x = 1;\n * ```\n */\nfn f() {}\n'
check block_inner_tilde fail '/*! ~~~\n * let x = 1;\n * ~~~\n */\nfn f() {}\n'
check block_one_line    fail '/** ```let x = 1;``` */\nfn f() {}\n'
check block_fence_later fail '/**\n * prose\n *\n * ```\n * f();\n * ```\n */\nfn f() {}\n'
check block_glued_fence fail '/**```let x = 1; */\nfn f() {}\n'
check plain_prose       pass '/// plain prose\n//! more prose\nfn f() {}\n'
check block_prose       pass '/** plain prose\n * more prose\n */\n/*! also prose */\nfn f() {}\n'
check block_fence_code  pass '/** prose */\nfn f() { let s = "```"; }\n'
check plain_block_fence pass '/* ```\n * not a doc\n */\nfn f() {}\n'
check empty_block_code  pass '/**/\nfn f() { let s = "```"; }\n'
check block_star_close  pass '/** prose **/\nfn f() { let s = "```"; }\n'
check nested_block_fence fail '/** outer /* nested */\n * ```\n * let x = 1;\n * ```\n */\nfn f() {}\n'
check str_block_literal pass 'fn f() { let s = "/** ```"; }\n'
check str_inner_literal pass 'fn f() { let s = "/*! ~~~"; }\n'

if [ "$fails" -ne 0 ]; then
  echo "no-doctests fixtures: $fails failing"
  exit 1
fi
echo "no-doctests fixtures: OK"
