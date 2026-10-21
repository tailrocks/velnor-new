//! Adapter-grammar project tools (HD-4a): the JDK and dbt-fusion resolver functions.

use std::collections::BTreeSet;

/// Variable of the JDK declaration, whose grammar is the adapter-specific
/// HD-4a form instead of exact `X.Y.Z`.
pub const JAVA_TOOL_VARIABLE: &str = "VELNOR_TOOL_JAVA";

/// Variable of the dbt-fusion declaration, whose grammar is the adapter-specific
/// HD-4a form `X.Y.Z-preview.N` instead of exact `X.Y.Z`.
pub const DBT_FUSION_TOOL_VARIABLE: &str = "VELNOR_TOOL_DBT_FUSION";

/// Exact-grammar checker of the JDK declaration (HD-4a), appended only for jobs that reference the JDK.
///
/// `velnor_exact_java` accepts exactly
/// `<vendor>-<X.Y.Z[.N]*>[+<build>[.<n>]*][.LTS]` (three or more numeric parts,
/// as Oracle `GraalVM` `25.0.4.1.1`) over `[A-Za-z0-9.+-]`, so the
/// single-line `$GITHUB_ENV` write cannot be injected.
const JAVA_FUNCTIONS: &str = r#"velnor_exact_java() {
  v=$2
  case "$v" in
    ''|*[!A-Za-z0-9.+-]*) velnor_fail "$1 must declare an exact JDK such as oracle-graalvm-25.0.3, not '$(velnor_safe "$v")'" ;;
  esac
  rest=
  for vendor in temurin oracle-graalvm graalvm-community corretto zulu; do
    case "$v" in "$vendor"-*) rest=${v#"$vendor"-} ;; esac
  done
  [ -n "$rest" ] || velnor_fail "$1 must name a vendor (temurin, oracle-graalvm, graalvm-community, corretto, zulu), not '$(velnor_safe "$v")'"
  case "$rest" in *.LTS) rest=${rest%.LTS} ;; esac
  core=${rest%%+*}
  build=
  case "$rest" in *+*) build=${rest#*+}; [ -n "$build" ] || velnor_fail "$1 has an empty build in '$(velnor_safe "$v")'" ;; esac
  case "$core" in
    ''|*[!0-9.]*|.*|*.|*..*|0[0-9]*|*.0[0-9]*) velnor_fail "$1 must declare an exact JDK version of three or more numeric parts, not '$(velnor_safe "$v")'" ;;
    *.*.*) ;;
    *) velnor_fail "$1 must declare an exact JDK version of three or more numeric parts, not '$(velnor_safe "$v")'" ;;
  esac
  case "$build" in
    '') ;;
    *[!0-9.]*|.*|*.|*..*) velnor_fail "$1 has a malformed build in '$(velnor_safe "$v")'" ;;
  esac
}
"#;

/// dbt-fusion checker (HD-4a), appended only for jobs that reference it.
///
/// `velnor_exact_fusion` accepts exactly `X.Y.Z-preview.N` over `[A-Za-z0-9.-]`,
/// so the single-line `$GITHUB_ENV` write cannot be injected.
const FUSION_FUNCTIONS: &str = r#"velnor_exact_fusion() {
  v=$2
  case "$v" in
    ''|*[!A-Za-z0-9.-]*) velnor_fail "$1 must declare an exact dbt-fusion such as 2.0.0-preview.219, not '$(velnor_safe "$v")'" ;;
  esac
  case "$v" in *-preview.*) ;; *) velnor_fail "$1 must be X.Y.Z-preview.N, not '$(velnor_safe "$v")'" ;; esac
  core=${v%%-preview.*}
  number=${v#*-preview.}
  velnor_exact "$1" "$core"
  case "$number" in
    ''|*[!0-9]*|0?*) velnor_fail "$1 has a malformed preview number in '$(velnor_safe "$v")'" ;;
  esac
}
"#;

/// Shared per-tool loop for adapter-grammar tools: reads every matching
/// `[tools]` row, applies the checker and writes one `$GITHUB_ENV` line.
const GRAMMAR_TOOL_TEMPLATE: &str = r#"velnor_tool_@KIND@() {
  name=$1
  shift
  resolved=
  origin=
  while IFS="$velnor_tab" read -r file key value; do
    matched=0
    for want in "$@"; do
      if [ "$key" = "$want" ]; then matched=1; fi
    done
    if [ "$matched" = 0 ]; then continue; fi
    @CHECK@ "$file $key" "$value"
    if [ -n "$resolved" ] && [ "$resolved" != "$value" ]; then
      velnor_fail "$name: $file $key declares $value, $origin declares $resolved"
    fi
    if [ -z "$resolved" ]; then origin="$file $key"; fi
    resolved=$value
  done < "$velnor_db"
  [ -n "$resolved" ] || velnor_fail "$name: the project config no longer declares this tool; regenerate the workflow"
  printf '%s=%s\n' "$name" "$resolved" >> "$GITHUB_ENV"
  printf 'velnor: %s=%s\n' "$name" "$resolved"
}
"#;

/// Tool variables resolved under an adapter grammar: `(variable, kind, checker)`.
pub const GRAMMAR_TOOLS: [(&str, &str, &str); 2] = [
    (JAVA_TOOL_VARIABLE, "java", "velnor_exact_java"),
    (DBT_FUSION_TOOL_VARIABLE, "fusion", "velnor_exact_fusion"),
];

/// Append the checker and loop of every adapter-grammar tool the job references.
pub(super) fn push_grammar_functions(script: &mut String, tools: &BTreeSet<&str>) {
    for (variable, kind, checker) in GRAMMAR_TOOLS {
        if !tools.contains(variable) {
            continue;
        }
        script.push_str(if kind == "java" {
            JAVA_FUNCTIONS
        } else {
            FUSION_FUNCTIONS
        });
        script.push_str(
            &GRAMMAR_TOOL_TEMPLATE
                .replace("@KIND@", kind)
                .replace("@CHECK@", checker),
        );
    }
}
