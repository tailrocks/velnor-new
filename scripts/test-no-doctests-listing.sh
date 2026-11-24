#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
checker="$repo/scripts/check-no-doctests.sh"
work="$(mktemp -d "${TMPDIR:-/tmp}/no-doctests-listing.XXXXXX")"
trap 'rm -rf "$work"' EXIT

fails=0
default_feature='default = ["on"]\n'
fence='```'
check() {
  local name="$1" expect="$2" lib="$3" dir="$work/$1"
  shift 3
  mkdir -p "$dir/fx/src"
  cp "$repo/rust-toolchain.toml" "$dir/rust-toolchain.toml"
  printf '[workspace]\nmembers = ["fx"]\nresolver = "3"\n' > "$dir/Cargo.toml"
  printf '[package]\nname = "fx"\nversion = "0.0.0"\nedition = "2024"\n\n[lib]\ndoctest = false\n\n[features]\n%bon = []\ngated = []\n%b' "$default_feature" "${extra_manifest:-}" > "$dir/fx/Cargo.toml"
  printf '%b' "$lib" > "$dir/fx/src/lib.rs"
  printf '%s\nlet x = 1;\n%s\n' "$fence" "$fence" > "$dir/fx/src/fence.md"
  while [ "$#" -ge 2 ]; do
    mkdir -p "$(dirname "$dir/fx/$1")"
    printf '%b' "$2" > "$dir/fx/$1"
    shift 2
  done
  if [ -e "$dir/Cargo.lock" ]; then
    echo "FAIL $name: fixture unexpectedly has a Cargo.lock"
    fails=$((fails + 1))
  fi
  if out="$("$BASH" "$checker" "$dir" 2>&1)"; then
    got=pass
  elif printf '%s\n' "$out" | grep -q 'rustdoc lists the doctests above'; then
    got=fail
  else
    got=broken
  fi
  if [ "$got" = pass ] && [ -n "${passes:-}" ] && ! printf '%s\n' "$out" | grep -q "(${passes} feature passes)"; then
    echo "FAIL $name: expected ${passes} feature passes"
    printf '%s\n' "$out"
    fails=$((fails + 1))
  elif [ "$got" != "$expect" ]; then
    echo "FAIL $name: expected $expect, got $got"
    printf '%s\n' "$out"
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
check bin_target_fence  fail 'pub fn f() {}\n' src/main.rs '/// ```\n/// let x = 1;\n/// ```\nfn main() {}\n'
check example_fence     fail 'pub fn f() {}\n' examples/e.rs '/// ```\n/// let x = 1;\n/// ```\nfn main() {}\n'
check not_feature_fence fail '#[cfg(not(feature = "gated"))]\n/// ```\n/// let x = 1;\n/// ```\nfn f() {}\n'
check default_only_fence fail '#[cfg(all(feature = "on", not(feature = "gated")))]\n/// ```\n/// let x = 1;\n/// ```\nfn f() {}\n'
check no_default_only_fence fail '#[cfg(not(feature = "on"))]\n/// ```\n/// let x = 1;\n/// ```\nfn f() {}\n'
check plain_prose       pass '//! inner prose\n/// plain prose\nfn f() {}\n'
check text_fence        pass '/// ```text\n/// not rust\n/// ```\nfn f() {}\n'
check prose_cont_line   pass '/// prose\n///     let x = 1;\nfn f() {}\n'
check plain_block_fence pass '/* ```\n * not a doc\n */\nfn f() {}\n'
check build_warning     pass 'pub fn f() {}\n' build.rs 'fn main() { println!("cargo:warning=this is not a doctest: test"); }\n'
check discovery_failure broken 'fn f( {\n'
check string_fence      pass 'fn f() { let s = "```"; let _ = s; }\n'
RUSTDOCFLAGS='--test-args foo' check inherited_test_args broken 'pub fn f() {}\n'
extra_manifest='\n[dependencies]\npm = { path = "../pm" }\n' check proc_macro_prints_doctest_line pass 'pub fn f() {}\npm::noisy!();\n' ../pm/Cargo.toml '[package]\nname = "pm"\nversion = "0.0.0"\nedition = "2024"\n\n[lib]\nproc-macro = true\ndoctest = false\n' ../pm/src/lib.rs 'use proc_macro::TokenStream;\n#[proc_macro]\npub fn noisy(_: TokenStream) -> TokenStream {\n    println!("fx/src/lib.rs - f (line 1): test");\n    TokenStream::new()\n}\n'
passes=3 check three_passes_with_a_default_feature pass 'pub fn f() {}\n'
default_feature='' passes=2 check two_passes_without_a_default_feature pass 'pub fn f() {}\n'
default_feature='' check no_default_feature_fence fail '#[cfg(feature = "on")]\n/// ```\n/// let x = 1;\n/// ```\nfn f() {}\n'

exit "$fails"
