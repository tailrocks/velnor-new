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
check indent_no_blank   fail '/// - item\n///     onto the next line\nfn f() {}\n'
check block_outer_fence fail '/** ```\n * let x = 1;\n * ```\n */\nfn f() {}\n'
check block_inner_tilde fail '/*! ~~~\n * let x = 1;\n * ~~~\n */\nfn f() {}\n'
check block_one_line    pass '/** ```let x = 1;``` */\nfn f() {}\n'
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
check shebang_attr_sep fail '#! /* separator */ [allow(dead_code)] /** ```\n */\nfn f() {}\n'
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
check attr_fence_start pass '#[doc = "```foo``` inline"]\npub fn f() {}\n'
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
check attr_escape_inline_pass pass '#[doc = "Use \\x60\\x60\\x60foo inline"]\npub fn f() {}\n'
check attr_gap_comment_fence fail '#[doc /* gap */ = "```\\nassert_eq!(1, 1);\\n```"]\npub fn f() {}\n'
check attr_gap_comment_include fail '#[doc /* gap */ = include_str!("README.md")]\npub fn f() {}\n'
check attr_gap_line_comment_fence fail '#[doc // gap\n = "```\\nassert_eq!(1, 1);\\n```"]\npub fn f() {}\n'
check footnote_block_unclosed fail '/**\n * prose\n *\n * [^x]: ```\n * assert_eq!(1, 2);\n */\nfn f() {}\n'
check footnote_line_unclosed fail '/// [^x]: ```\n/// assert_eq!(1, 2);\nfn f() {}\n'
check footnote_prose_pass pass '/// [^x]: plain footnote text\n/// assert_eq!(1, 2);\nfn f() {}\n'
check attr_gap_hash_fence fail '# /* gap */ [doc = "```\\nassert_eq!(1, 1);\\n```"]\npub fn f() {}\n'
check shebang_gap_attr  fail '#!/* g */[doc = "```\\nassert_eq!(1, 1);\\n```"]\npub fn f() {}\n'
check line_tab_indent   fail '/// text\n///\n/// \tassert_eq!(1, 1);\npub fn f() {}\n'
check attr_tab_indent   fail '#[doc = "prose\\n\\n\\tassert_eq!(1, 1);"]\npub fn f() {}\n'
check block_tab_indent  fail '/**\n * prose\n *\n * \tassert_eq!(1, 1);\n */\npub fn f() {}\n'
check attr_cr_fence     fail '#![doc = "prose\\r```\\rassert_eq!(1, 1);\\r```"]\npub fn f() {}\n'
check attr_cr_indent    fail '#![doc = "prose\\r\\r    assert_eq!(1, 1);\\r"]\npub fn f() {}\n'
check line_list_indent  fail '/// -     assert_eq!(1, 1);\npub fn f() {}\n'
check block_list_indent fail '/**\n * -     assert_eq!(1, 1);\n */\npub fn f() {}\n'
check attr_list_indent  fail '#[doc = "-     assert_eq!(1, 1);"]\npub fn f() {}\n'
check line_tab_no_space pass '/// text\n///\n///\tassert_eq!(1, 1);\npub fn f() {}\n'
check block_tab_no_indent pass '/**\n * prose\n *\n *\tassert_eq!(1, 1);\n */\npub fn f() {}\n'
check line_list_prose   pass '/// -    not code\npub fn f() {}\n'
check task_fence_line  fail '/// - [ ] ```\n///   let x = 1;\npub fn f() {}\n'
check task_fence_block fail '/**\n * - [ ] ```\n *   let x = 1;\n */\npub fn f() {}\n'
check dash_adjacent_prose pass '/// Doc.\n/// -- text\n/// --     text\npub fn f() {}\n'
check line_tab_then_space pass '/// text\n///\n///\t assert_eq!(1, 1);\npub fn f() {}\n'
check line_tab_two_tabs fail '/// text\n///\n///\t\tassert_eq!(1, 1);\npub fn f() {}\n'
check line_tab_then_four fail '/// text\n///\n///\t    assert_eq!(1, 1);\npub fn f() {}\n'
check quote_continuation pass '/// > prose\n/// >     assert_eq!(1, 1);\nfn f() {}\n'
check quote_continuation_wide pass '/// > prose\n/// >       assert_eq!(1, 1);\nfn f() {}\n'
check quote_continuation_chain pass '/// > prose\n/// >     assert_eq!(1, 1);\n/// > more\nfn f() {}\n'
check prose_continuation pass '/// prose\n///         assert_eq!(1, 1);\nfn f() {}\n'
check quote_depth_change fail '/// prose\n/// >     assert_eq!(1, 1);\nfn f() {}\n'
check setext_rule_indent fail '/// prose\n/// ---\n///     assert_eq!(1, 1);\nfn f() {}\n'
check setext_eq_indent fail '/// prose\n/// ===\n///     assert_eq!(1, 1);\nfn f() {}\n'
check thematic_break_indent fail '/// prose\n/// ***\n///     assert_eq!(1, 1);\nfn f() {}\n'
check html_block_fence pass '/// <div>\n/// > ```\n/// > assert_eq!(1, 1);\n/// > ```\n/// </div>\nfn f() {}\n'
check html_block_indent pass '/// <div>\n///     assert_eq!(1, 1);\n/// </div>\nfn f() {}\n'
check html_after_prose pass '/// prose\n/// <div>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check html_blank_ends fail '/// <div>\n///\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check html_span_block_start pass '/// <span>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check html_span_after_prose fail '/// prose\n/// <span>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check html_one_line_comment fail '/// <!-- comment -->\n///     assert_eq!(1, 2);\nfn f() {}\n'
check html_one_line_script fail '/// <script></script>\n///     assert_eq!(1, 2);\nfn f() {}\n'
check html_one_line_prose fail '/// prose\n/// <!-- comment -->\n///     assert_eq!(1, 2);\nfn f() {}\n'
check html_multiline_close fail '/// <!--\n/// -->\n///     assert_eq!(1, 2);\nfn f() {}\n'
check html_multiline_indent pass '/// <!--\n///     assert_eq!(1, 2);\n/// -->\nfn f() {}\n'
check html_one_line_quote fail '/// > <!-- c -->\n/// >     assert_eq!(1, 2);\nfn f() {}\n'
check html_pre_raw_fence pass '/// <pre>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\n/// </pre>\nfn f() {}\n'
check quote_code_span pass '/// > ```foo``` inline\nfn f() {}\n'
check line_code_span_start pass '/// ```foo``` inline\nfn f() {}\n'
check pre_bare_fence    fail '/// <pre>\n/// </pre>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check script_bare_fence fail '/// <script>\n/// </script>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check style_bare_fence  fail '/// <style>\n/// </style>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check textarea_bare_fence fail '/// <textarea>\n/// </textarea>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check pre_class_bare_fence fail '/// <pre class="x">\n/// </pre>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check pre_three_line_fence fail '/// <pre>\n/// assert_eq!(1, 1);\n/// </pre>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check pre_blank_fence   fail '/// <pre>\n///\n/// </pre>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check bq_pre_unquoted_fence fail '/// > <pre>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check bq_comment_unquoted_fence fail '/// > <!--\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check bq_script_unquoted_fence fail '/// > <script>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check bq_html_unquoted_fence fail '/// > <div>\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'
check quote_raw_continuation pass '/// > <!--\n/// > ```\n/// > assert_eq!(1, 1);\n/// > -->\nfn f() {}\n'
check indent_then_fence fail '/// prose\n///     let x = 1;\n/// ```\n/// assert_eq!(1, 1);\n/// ```\nfn f() {}\n'

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
