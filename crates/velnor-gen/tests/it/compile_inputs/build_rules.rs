// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::build_output::{OUT, UNRESOLVED, outcome, owed};
use crate::facts::TestResult;

const RESOLVED: &str = "const A: &str = include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/x.md\"));\nconst B: &str = include_str!(concat!(env!(\"OUT_DIR\"), \"/g.md\"));";
const CHANGED: &str = "println!(\"cargo:rerun-if-changed=build.rs\");";

#[test]
fn a_rerun_if_changed_declares_only_as_a_top_level_print_of_main() -> TestResult {
    for (what, script) in [
        (
            "cond",
            "fn main() { if std::env::var(\"X\").is_ok() { println!(\"cargo:rerun-if-changed=build.rs\"); } }",
        ),
        (
            "tofile",
            "fn main() { let mut f = std::fs::File::create(\"o\").unwrap(); writeln!(f, \"cargo:rerun-if-changed=build.rs\").unwrap(); }",
        ),
        (
            "write",
            "fn main() { write!(std::io::stdout(), \"cargo:rerun-if-changed=build.rs\").unwrap(); }",
        ),
        (
            "match",
            "fn main() { match 1 { _ => println!(\"cargo:rerun-if-changed=build.rs\") } }",
        ),
        (
            "loop",
            "fn main() { for _ in 0..1 { println!(\"cargo:rerun-if-changed=build.rs\"); } }",
        ),
        (
            "closure",
            "fn main() { let f = || println!(\"cargo:rerun-if-changed=build.rs\"); f(); }",
        ),
        (
            "other function",
            "fn helper() { println!(\"cargo:rerun-if-changed=build.rs\"); }\nfn main() { helper(); }",
        ),
        (
            "a later statement of an if block",
            "fn main() { if std::env::var(\"X\").is_ok() { let _ = 1; println!(\"cargo:rerun-if-changed=build.rs\"); } }",
        ),
        ("no main", "fn helper() {}"),
        (
            "one of two mains silent",
            "#[cfg(unix)]\nfn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }\n#[cfg(not(unix))]\nfn main() {}",
        ),
    ] {
        assert_eq!(outcome(OUT, &[("c", script)])?, owed(&UNRESOLVED), "{what}");
    }
    for (what, script) in [
        (
            "print",
            "fn main() { print!(\"cargo:rerun-if-changed=build.rs\"); }",
        ),
        (
            "after an if block",
            "fn main() { if std::env::var(\"X\").is_ok() { return; } println!(\"cargo:rerun-if-changed=build.rs\"); }",
        ),
        (
            "a result",
            "fn main() -> Result<(), Box<dyn std::error::Error>> { println!(\"cargo:rerun-if-changed=build.rs\"); Ok(()) }",
        ),
        (
            "after a helper",
            "fn helper() {}\nfn main() { helper(); println!(\"cargo:rerun-if-changed=build.rs\"); }",
        ),
        (
            "a function named main inside another function",
            "fn helper() { fn main() {} }\nfn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }",
        ),
        (
            "the cargo:: spelling",
            "fn main() { println!(\"cargo::rerun-if-changed=build.rs\"); }",
        ),
    ] {
        assert_eq!(outcome(OUT, &[("c", script)])?, Vec::new(), "{what}");
    }
    Ok(())
}

fn keyed(prelude: &str, line: &str) -> String {
    format!("{prelude}\nfn main() {{ {CHANGED} {line} }}")
}

#[test]
fn a_rustc_env_key_is_read_from_any_literal_and_a_formatted_key_fails_closed() -> TestResult {
    let reserved = "const KEY: &str = \"OUT_DIR\";";
    for (what, prelude, line) in [
        (
            "bnl",
            "",
            "println!(\"\\ncargo:rustc-env=CARGO_MANIFEST_DIR=/e\");",
        ),
        (
            "after other text",
            "",
            "println!(\"cargo:warning=x\\ncargo:rustc-env=OUT_DIR=/e\");",
        ),
        (
            "bfmt positional",
            "",
            "println!(\"cargo:rustc-env={}={}\", \"OUT_DIR\", \"/e\");",
        ),
        (
            "a const key",
            reserved,
            "println!(\"cargo:rustc-env={KEY}=/e\");",
        ),
        (
            "an unresolvable name",
            "",
            "let k = \"OUT_DIR\"; println!(\"cargo:rustc-env={k}=/e\");",
        ),
        (
            "a partial key",
            "",
            "println!(\"cargo:rustc-env=OUT_{}=/e\", \"DIR\");",
        ),
        (
            "a bound literal",
            "",
            "let line = \"cargo:rustc-env=OUT_DIR=/e\"; println!(\"{line}\");",
        ),
        (
            "a second directive in one literal",
            "",
            "println!(\"cargo:rustc-env=A=b\\ncargo:rustc-env=OUT_DIR=/e\");",
        ),
        (
            "no closing equals sign",
            "",
            "println!(\"cargo:rustc-env=OUT_DIR\");",
        ),
    ] {
        let script = keyed(prelude, line);
        for source in [RESOLVED, ""] {
            assert_eq!(
                outcome(source, &[("c", &script)])?,
                owed(&UNRESOLVED),
                "{what}"
            );
        }
    }
    let harmless = keyed(
        "const OTHER: &str = \"GIT_HASH\";",
        "println!(\"cargo:rustc-env={OTHER}=abc\");",
    );
    assert_eq!(outcome(RESOLVED, &[("c", &harmless)])?, Vec::new());
    Ok(())
}
