//! Frontmatter parser and validation cases.

use super::*;
use std::path::Path;
use std::process::{Command, Output};

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn run_validator(root: &Path) -> Output {
    Command::new("node")
        .arg("-e")
        .arg(FRONTMATTER_ID_SCRIPT)
        .current_dir(root)
        .output()
        .expect("run frontmatter validator")
}

fn write_skill(root: &Path, name: &str, frontmatter: &str) {
    let path = root.join("skills").join(name).join("SKILL.md");
    std::fs::create_dir_all(path.parent().expect("skill parent")).expect("create skill directory");
    let separator = if frontmatter.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    std::fs::write(path, format!("---\n{frontmatter}{separator}---\nBody.\n"))
        .expect("write skill file");
}

#[test]
fn frontmatter_parser_normalizes_multiline_yaml() {
    if !node_available() {
        return;
    }
    let temp = tempfile::tempdir().expect("temporary repository");
    let fixtures = [
        (
            "folded",
            "> # comment\n  first line\n  second line\n\n  second paragraph",
        ),
        ("literal", "|-2\n  first line\n  second line"),
        ("literal-paragraph", "|-\n  first line\n\n  second line"),
        ("literal-leading", "|-\n\n  first line"),
        ("quoted", "\"A \\\"quoted\\\" # value\" # trailing comment"),
        ("yaml-escaped", "\"Hello\\x20world\\u0021\\U0001F642\""),
        ("plain", "first line\n  second line\n\n  second paragraph"),
        ("modifier-order", ">-2\n  first line\n  second line"),
        ("literal-keep", "|+2\n  first line\n\n"),
    ];
    for (name, description) in fixtures {
        write_skill(
            temp.path(),
            name,
            &format!(
                "name: {name}\ndescription: {description}\nmetadata: {{author: team}}\nallowed-tools: [read, grep]"
            ),
        );
    }
    let instrumented = FRONTMATTER_ID_SCRIPT.replace(
        "const n = fm.get(\"name\");",
        "console.log(\"description:\" + JSON.stringify(fm.get(\"description\"))); const n = fm.get(\"name\");",
    );
    let output = Command::new("node")
        .arg("-e")
        .arg(instrumented)
        .current_dir(temp.path())
        .output()
        .expect("run frontmatter validator");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    for expected in [
        r#"description:"first line second line\nsecond paragraph""#,
        r#"description:"first line\nsecond line""#,
        r#"description:"first line\n\nsecond line""#,
        r#"description:"\nfirst line""#,
        r#"description:"A \"quoted\" # value""#,
        r#"description:"Hello world!🙂""#,
        r#"description:"first line second line""#,
        r#"description:"first line\n\n\n""#,
    ] {
        assert!(
            stdout.contains(expected),
            "missing {expected:?} in {stdout}"
        );
    }
}

#[test]
fn frontmatter_plain_multiline_ignores_indented_comments() {
    if !node_available() {
        return;
    }
    let temp = tempfile::tempdir().expect("temporary repository");
    let comment = "x".repeat(1100);
    write_skill(
        temp.path(),
        "plain-comment",
        &format!(
            "name: plain-comment\ndescription: first line\n  # {comment}\n  second line # {comment}"
        ),
    );
    let instrumented = FRONTMATTER_ID_SCRIPT.replace(
        "const n = fm.get(\"name\");",
        "console.log(\"description:\" + JSON.stringify(fm.get(\"description\"))); const n = fm.get(\"name\");",
    );
    let output = Command::new("node")
        .arg("-e")
        .arg(instrumented)
        .current_dir(temp.path())
        .output()
        .expect("run frontmatter validator");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains(r#"description:"first line second line""#));
}

#[test]
fn frontmatter_empty_value_accepts_plain_and_quoted_scalars() {
    if !node_available() {
        return;
    }
    let temp = tempfile::tempdir().expect("temporary repository");
    write_skill(
        temp.path(),
        "plain-child",
        "name: plain-child\ndescription:\n  first line\n  second line",
    );
    write_skill(
        temp.path(),
        "double-child",
        "name: double-child\ndescription:\n  \"Use: tool\"",
    );
    write_skill(
        temp.path(),
        "single-child",
        "name: single-child\ndescription:\n  'Use: tool'",
    );
    let instrumented = FRONTMATTER_ID_SCRIPT.replace(
        "const n = fm.get(\"name\");",
        "console.log(\"description:\" + JSON.stringify(fm.get(\"description\"))); const n = fm.get(\"name\");",
    );
    let output = Command::new("node")
        .arg("-e")
        .arg(instrumented)
        .current_dir(temp.path())
        .output()
        .expect("run frontmatter validator");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains(r#"description:"first line second line""#));
    assert_eq!(stdout.matches(r#"description:"Use: tool""#).count(), 2);
}

#[test]
fn frontmatter_parser_rejects_duplicate_keys_and_bad_scalars() {
    if !node_available() {
        return;
    }
    let temp = tempfile::tempdir().expect("temporary repository");
    write_skill(
        temp.path(),
        "folded",
        "name: folded\ndescription: first\ndescription: second",
    );
    let duplicate = run_validator(temp.path());
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stdout).contains("duplicate-frontmatter-key"));

    for (name, value) in [
        ("unclosed", "\"not closed"),
        ("missing-comment-separator", "\"valid\"#junk"),
        ("missing-single-quote-comment-separator", "'valid'#junk"),
        ("invalid-yaml-escape", "\"bad\\qescape\""),
        ("collection", "[]"),
        ("hex-number", "0x12"),
        ("octal-number", "0o12"),
        ("binary-number", "0b10"),
        ("multiline-quoted", "\"first line\n  second line\""),
        ("bad-indent", "|2\n x"),
        ("tagged-sequence", "!!seq [item]"),
    ] {
        write_skill(
            temp.path(),
            name,
            &format!("name: {name}\ndescription: {value}"),
        );
    }
    let malformed = run_validator(temp.path());
    let stdout = String::from_utf8_lossy(&malformed.stdout);
    assert!(!malformed.status.success());
    for expected in [
        "bad-frontmatter-scalar",
        "non-string-frontmatter-scalar",
        "bad-frontmatter-indent",
        "unsupported-frontmatter-alias-or-tag",
    ] {
        assert!(stdout.contains(expected), "missing {expected} in {stdout}");
    }
}

fn assert_rejected_indented_scalar(name: &str, scalar: &str) {
    let temp = tempfile::tempdir().expect("temporary repository");
    write_skill(
        temp.path(),
        name,
        &format!("name: {name}\ndescription:\n  {scalar}"),
    );
    let output = run_validator(temp.path());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(
            "non-string-frontmatter-scalar:skills/{name}/SKILL.md"
        )),
        "indented non-string scalar accepted: {scalar}; output: {stdout}"
    );
}

#[test]
fn frontmatter_parser_rejects_aliases_and_block_collections() {
    if !node_available() {
        return;
    }
    let temp = tempfile::tempdir().expect("temporary repository");
    let long_value = "x".repeat(1025);
    write_skill(
        temp.path(),
        "alias",
        &format!("name: alias\npayload: &long {long_value}\ndescription: *long"),
    );
    let alias = run_validator(temp.path());
    assert!(!alias.status.success());
    assert!(
        String::from_utf8_lossy(&alias.stdout).contains("unsupported-frontmatter-alias-or-tag")
    );
    write_skill(
        temp.path(),
        "block-alias",
        &format!("name: block-alias\npayload: &long {long_value}\ndescription:\n  *long"),
    );
    assert!(!run_validator(temp.path()).status.success());

    write_skill(
        temp.path(),
        "collection",
        "name: collection\ndescription:\n  - one",
    );
    let collection = run_validator(temp.path());
    assert!(!collection.status.success());
    assert!(String::from_utf8_lossy(&collection.stdout).contains("non-string-frontmatter-scalar"));

    write_skill(
        temp.path(),
        "blank-collection",
        "name: blank-collection\ndescription:\n\n  - one",
    );
    let blank_collection = run_validator(temp.path());
    assert!(!blank_collection.status.success());
    assert!(
        String::from_utf8_lossy(&blank_collection.stdout).contains("non-string-frontmatter-scalar")
    );
    write_skill(
        temp.path(),
        "quoted-map",
        "name: quoted-map\ndescription:\n  \"text\": one",
    );
    assert!(!run_validator(temp.path()).status.success());
    for (name, scalar) in [
        ("eol-map", "text:"),
        ("boolean", "true"),
        ("number", "12"),
        ("hex-number", "0x12"),
        ("octal-number", "0o12"),
        ("binary-number", "0b10"),
        ("null", "null"),
    ] {
        assert_rejected_indented_scalar(name, scalar);
    }
    write_skill(
        temp.path(),
        "bare-sequence",
        "name: bare-sequence\ndescription:\n  -",
    );
    assert!(!run_validator(temp.path()).status.success());
    write_skill(
        temp.path(),
        "flow-sequence",
        "name: flow-sequence\ndescription:\n  [one]",
    );
    assert!(!run_validator(temp.path()).status.success());
    write_skill(
        temp.path(),
        "tagged-flow-sequence",
        "name: tagged-flow-sequence\ndescription:\n  !!seq [one]",
    );
    assert!(!run_validator(temp.path()).status.success());
}

#[test]
fn frontmatter_empty_plain_scalars_are_checked_after_comment_stripping_and_folding() {
    if !node_available() {
        return;
    }
    for (name, value, valid) in [
        ("boolean-comment", "true # comment", false),
        ("number-comment", "12 # comment", false),
        ("null-comment", "null # comment", false),
        ("boolean-continuation", "true\n  story about tools", true),
        ("number-continuation", "12\n  tools for analysis", true),
    ] {
        let temp = tempfile::tempdir().expect("temporary repository");
        write_skill(
            temp.path(),
            name,
            &format!("name: {name}\ndescription:\n  {value}"),
        );
        let instrumented = FRONTMATTER_ID_SCRIPT.replace(
            "const n = fm.get(\"name\");",
            "console.log(\"description:\" + JSON.stringify(fm.get(\"description\"))); const n = fm.get(\"name\");",
        );
        let output = Command::new("node")
            .arg("-e")
            .arg(instrumented)
            .current_dir(temp.path())
            .output()
            .expect("run frontmatter validator");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert_eq!(
            output.status.success(),
            valid,
            "unexpected validation result for {value:?}: {stdout}"
        );
        if valid {
            assert!(
                stdout.contains("description:"),
                "missing normalized value: {stdout}"
            );
        } else {
            assert!(
                stdout.contains(&format!(
                    "non-string-frontmatter-scalar:skills/{name}/SKILL.md"
                )),
                "missing type error for {value:?}: {stdout}"
            );
        }
    }
}
