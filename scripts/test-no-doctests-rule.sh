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
  awk '/^  - id: no-doctests/ {on = 1; print; next} /^  - id: / {on = 0} on && /^[^ #]/ {exit} on {print}' "$config"
} > "$rule"
grep -q 'id: no-doctests$' "$rule" || { echo "no-doctests rule not found in $config" >&2; exit 2; }
grep -q 'id: no-doctests-scanner' "$rule" || { echo "no-doctests-scanner rule not found in $config" >&2; exit 2; }

fails=0
# check <name> <fail|pass> <file content with printf %b escapes>
check() {
  local name="$1" expect="$2" dir="$work/$1"
  mkdir -p "$dir/crates/fx/src" "$dir/scripts"
  cp "$repo/scripts/no-doctests.py" "$dir/scripts/"
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
check indent_list_cont  pass '/// - item\n///     onto the next line\nfn f() {}\n'
check prose_cont_line   pass '/// prose\n///     let x = 1;\nfn f() {}\n'
check prose_cont_three  pass '/// one\n/// two\n///     let x = 1;\nfn f() {}\n'
check prose_cont_wide   pass '/// prose\n///       let x = 1;\nfn f() {}\n'
check prose_cont_block  pass '/**\n * prose\n *     let x = 1;\n */\nfn f() {}\n'
check prose_cont_attr   pass '#[doc = "prose\n    let x = 1;"]\nfn f() {}\n'
check quote_lazy_cont   pass '/// > quote\n///     let x = 1;\nfn f() {}\n'
check hr_indent         fail '/// ---\n///     let x = 1;\nfn f() {}\n'
check list5_bare        fail '/// -     let x = 1;\nfn f() {}\n'
check list5_after_prose fail '/// text\n/// -     let x = 1;\nfn f() {}\n'
check list5_num         fail '/// text\n/// 1.     let x = 1;\nfn f() {}\n'
check list5_star        fail '/// *     let x = 1;\nfn f() {}\n'
check list5_plus        fail '/// +     let x = 1;\nfn f() {}\n'
check list5_paren       fail '/// 1)     let x = 1;\nfn f() {}\n'
check list5_quoted      fail '/// >     - let x = 1;\nfn f() {}\n'
check list5_block       fail '/**\n * text\n * -     let x = 1;\n */\nfn f() {}\n'
check list5_attr        fail '#[doc = "text\n-     let x = 1;"]\nfn f() {}\n'
check quote5_after_prose fail '/// text\n/// >     let x = 1;\nfn f() {}\n'
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
check unicode_ident   pass 'fn café() {}\n/** plain prose */\nfn f() {}\n'
check shebang_block    pass '#!/usr/bin/env run /** ```\nfn f() {}\n'
check block_indent_code fail '/**\n * prose\n *\n *     let x = 1;\n */\nfn f() {}\n'
check block_inline_span pass '/** Use ```foo``` as an inline code span. */\nfn f() {}\n'
check block_inline_mid  pass '/**\n * Call ~~~ here and ``` there.\n */\nfn f() {}\n'
check block_tilde_fence fail '/**\n * prose\n * ~~~\n * f();\n * ~~~\n */\nfn f() {}\n'
check str_line_fence    pass 'fn f() { let s = "\n/// ```\n"; }\n'
check raw_str_line_fence pass 'const S: &str = r#"\n/// ```\n"#;\nfn f() {}\n'
check str_line_indent   pass 'fn f() { let s = "\n///     code\n"; }\n'
check line_inline_span  pass '/// Use ```foo``` as code.\nfn f() {}\n'
check str_slashes_fence fail 'fn f() { let u = "http://x"; }\n/// ```\n/// f();\n/// ```\nfn g() {}\n'
check raw_then_fence    fail 'const S: &str = r#"x"#;\n/// ```\nfn g() {}\n'
check attr_in_string    pass 'fn f() { let s = "#[doc = include_str!(x)]"; }\n'
check attr_after_string fail 'fn f() { let s = "a"; }\n#[doc = include_str!(README.md)]\nfn g() {}\n'
check raw_attr_body    fail '#[doc = r"```\nassert_eq!(1, 2);\n```"]\npub fn f() {}\n'
check raw_hash_attr    fail '#[doc = r#"```\nx\n```"#]\npub fn f() {}\n'
check raw_inner_attr   fail '#![doc = r"```\nx\n```"]\nfn f() {}\n'
check cfg_attr_plain   fail '#[cfg_attr(docsrs, doc = "```\nx\n```")]\npub fn f() {}\n'
check cfg_attr_raw     fail '#[cfg_attr(docsrs, doc = r"```\nx\n```")]\npub fn f() {}\n'
check ws_hash_bracket  fail '# [doc = "```\nx\n```"]\npub fn f() {}\n'
check ws_in_bracket    fail '#[ doc = "```\nx\n```" ]\npub fn f() {}\n'
check ws_inner_bang    fail '#! [doc = "```\nx\n```"]\npub fn f() {}\n'
check attr_inline_span pass '#[doc = "Use ```foo``` inline"]\npub fn f() {}\n'
check attr_fence_start fail '#[doc = "```foo``` inline"]\npub fn f() {}\n'
check let_doc_string   pass 'fn f() { let doc = "```"; }\n'
check attr_indent4    fail '#![doc = "Prose\n\n    let x = 1;\n"]\nfn f() {}\n'
check attr_nested_incl fail '#![doc = concat!("intro", include_str!("README.md"))]\nfn f() {}\n'
check bq_line_fence    fail '/// > ```\n/// let x = 1;\n/// > ```\nfn f() {}\n'
check bq_nospace       fail '/// >```\n/// let x = 1;\n/// >```\nfn f() {}\n'
check bq_bang          fail '//! > ```\n//! let x = 1;\n//! > ```\n'
check bq_block_fence   fail '/**\n * > ```\n * let x = 1;\n * > ```\n */\nfn f() {}\n'
check bq_nested_line   fail '/// > > ```\n/// let x = 1;\n/// > > ```\nfn f() {}\n'
check bq_nested_block  fail '/**\n * > > ```\n * let x = 1;\n */\nfn f() {}\n'
check bq_tilde_line    fail '/// > ~~~\n/// let x = 1;\n/// > ~~~\nfn f() {}\n'
check bq_tilde_block   fail '/**\n * > ~~~\n * let x = 1;\n */\nfn f() {}\n'
check bq_attr_fence    fail '#[doc = "> ```\nlet x = 1;\n> ```"]\nfn f() {}\n'
check bq_attr_nested   fail '#[doc = "> > ```\nlet x = 1;"]\nfn f() {}\n'
check bq_attr_tilde    fail '#[doc = "> ~~~\nlet x = 1;"]\nfn f() {}\n'
check bq_indent_line   fail '/// >     let x = 1;\nfn f() {}\n'
check bq_indent_block  fail '/**\n * >     let x = 1;\n */\nfn f() {}\n'
check bq_indent_attr   fail '#[doc = ">     let x = 1;"]\nfn f() {}\n'
check list_dash_line   fail '/// - ```\n///   let x = 1;\nfn f() {}\n'
check list_star_line   fail '/// * ```\n///   let x = 1;\nfn f() {}\n'
check list_plus_line   fail '/// + ```\n///   let x = 1;\nfn f() {}\n'
check list_num_line    fail '/// 1. ```\n///    let x = 1;\nfn f() {}\n'
check list_paren_line  fail '/// 1) ```\n///    let x = 1;\nfn f() {}\n'
check list_tilde_line  fail '/// - ~~~\n///   let x = 1;\nfn f() {}\n'
check list_star_block  fail '/**\n * * ```\n *   let x = 1;\n */\nfn f() {}\n'
check list_num_block   fail '/**\n * 1. ```\n *    let x = 1;\n */\nfn f() {}\n'
check list_attr_fence  fail '#[doc = "- ```\nlet x = 1;"]\nfn f() {}\n'
check list_bq_fence    fail '/// > - ```\n/// >   let x = 1;\nfn f() {}\n'
check bq_prose         pass '/// > quoted prose\n/// > more prose\nfn f() {}\n'
check bq_block_prose   pass '/**\n * > quoted prose\n */\nfn f() {}\n'
check bq_indent4_prose pass '/// >    not code\nfn f() {}\n'
check list_prose       pass '/// - item text\n/// 1. step text\n/// * bullet\nfn f() {}\n'
check bq_inline_span   pass '/// > Use ```foo``` as code.\nfn f() {}\n'
check list_inline_span pass '/// - Call ``` here\nfn f() {}\n'
check bq_str_literal   pass 'fn f() { let s = "> ```"; }\n'
check list_str_literal pass 'fn f() { let s = "- ```\n    x"; }\n'
check attr_hex_escape_fence fail '#![doc = "prose\\x0a```\\x0aassert_eq!(1, 2);\\x0a```"]\npub fn f() {}\n'
check attr_unicode_escape_fence fail '#![doc = "prose\\u{a}```\\u{a}assert_eq!(1, 2);\\u{a}```"]\npub fn f() {}\n'
check attr_line_cont_fence fail '#![doc = "prose\\n\\\n```\\nassert_eq!(1, 2);\\n```"]\npub fn f() {}\n'
check attr_escape_backtick_fence fail '#[doc = "\\x60\\x60\\x60\\nlet x = 1;\\n\\x60\\x60\\x60"]\npub fn f() {}\n'
check attr_unicode_backtick_fence fail '#[doc = "\\u{60}\\u{60}\\u{60}\\nlet x = 1;\\n\\u{60}\\u{60}\\u{60}"]\npub fn f() {}\n'
check attr_escape_inline_pass pass '#[doc = "Use \\x60\\x60\\x60foo inline"]\npub fn f() {}\n'
check attr_gap_comment_fence fail '#[doc /* gap */ = "```\\nassert_eq!(1, 1);\\n```"]\npub fn f() {}\n'
check attr_gap_comment_include fail '#[doc /* gap */ = include_str!("README.md")]\npub fn f() {}\n'
check attr_gap_line_comment_fence fail '#[doc // gap\n = "```\\nassert_eq!(1, 1);\\n```"]\npub fn f() {}\n'
check footnote_block_unclosed fail '/**\n * prose\n *\n * [^x]: ```\n * assert_eq!(1, 2);\n */\nfn f() {}\n'
check footnote_line_unclosed fail '/// [^x]: ```\n/// assert_eq!(1, 2);\nfn f() {}\n'
check footnote_prose_pass pass '/// [^x]: plain footnote text\n/// assert_eq!(1, 2);\nfn f() {}\n'
check cfg_attr_meta_after_doc pass '#[cfg_attr(all(), doc = "prose", deprecated(note = "```not docs```"))]\npub fn f() {}\n'
check cfg_attr_doc_concat_comma fail '#[cfg_attr(all(), doc = concat!("prose", "```\\nx\\n```"))]\npub fn f() {}\n'
check cfg_attr_doc_then_fence fail '#[cfg_attr(all(), doc = "prose", doc = "```\\nx\\n```")]\npub fn f() {}\n'
check shebang_attr_sep fail '#! /* separator */ [allow(dead_code)] /** ```\n */\nfn f() {}\n'
check line_space_tab   fail '/// \tlet x = 1;\nfn f() {}\n'
check attr_tab_indent  fail '#[doc = "prose\\n\\n\\tassert_eq!(1, 1);"]\npub fn f() {}\n'
check attr_cr_fence    fail '#[doc = "prose\\r\\r```\\rassert_eq!(1, 1);\\r```"]\npub fn f() {}\n'
check attr_gap_opener  fail '#/* gap */[doc = "```\\nassert_eq!(1, 1);\\n```"]\npub fn f() {}\n'
check attr_empty_marker fail '#[doc = "1.\\n       assert_eq!(1, 1);"]\npub fn f() {}\n'
check cfg_doc_value    pass '#![allow(unexpected_cfgs)]\n#[cfg(doc = "```not documentation```")]\npub fn f() {}\n'
check attr_env_macro   fail '#[doc = env!("MYDOC")]\npub fn f() {}\n'
check attr_margin_pass pass '#[doc = "    assert!(true);"]\npub fn f() {}\n'
check attr_margin_fence fail '#[doc = "    assert!(true);\\n\\nprose\\n\\n        code();"]\npub fn f() {}\n'
check attr_gap_macro_bang fail '#[doc = env /* gap */ !("MYDOC")]\npub fn f() {}\n'
check attr_raw_ident_doc fail '#[r#doc = "```\\nassert_eq!(1, 1);\\n```"]\npub fn f() {}\n'

missing="$work/scanner_missing"
mkdir -p "$missing/crates/fx/src"
printf '%b' '/** ```\n * f();\n * ```\n */\nfn f() {}\n' > "$missing/crates/fx/src/lib.rs"
if "$alint_bin" check -c "$rule" "$missing" >/dev/null 2>&1; then got=pass; else got=fail; fi
if [ "$got" != fail ]; then
  echo "FAIL scanner_missing: expected fail, got $got"
  fails=$((fails + 1))
else
  echo "ok   scanner_missing (fail)"
fi

bad_utf8="$work/not_utf8"
mkdir -p "$bad_utf8/scripts"
cp "$repo/scripts/no-doctests.py" "$bad_utf8/scripts/"
printf 'fn f() { let s = "caf\xe9"; }\n' > "$bad_utf8/bad.rs"
out="$(python3 -I "$bad_utf8/scripts/no-doctests.py" "$bad_utf8/bad.rs" 2>&1)" && utf_rc=0 || utf_rc=$?
if [ "$utf_rc" -eq 1 ] && printf '%s' "$out" | grep -q 'not valid UTF-8' && ! printf '%s' "$out" | grep -q Traceback; then
  echo "ok   non_utf8_error (fail, one line)"
else
  echo "FAIL non_utf8_error: rc=$utf_rc output=$out"
  fails=$((fails + 1))
fi

if [ "$fails" -ne 0 ]; then
  echo "no-doctests fixtures: $fails failing"
  exit 1
fi
echo "no-doctests fixtures: OK"
