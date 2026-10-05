//! Tests for TOML multiline strings and array of tables.

use velnor_actions_mise::{CI_PROFILE_NAME, parse_cargo_wrapper, parse_nextest_config};

#[test]
fn multiline_basic_string_and_trimming() {
    let content = "[tasks.demo]\ncmd = \"\"\"\nline1\nline2\"\"\"\n";
    let wrapper = parse_cargo_wrapper(content).expect("parses");
    assert_eq!(wrapper, None);
}

#[test]
fn multiline_literal_string_in_tasks() {
    let content = "[tasks.demo]\nrun = '''\nset -euo pipefail\necho \"hi\"\n'''\n";
    let wrapper = parse_cargo_wrapper(content).expect("parses");
    assert_eq!(wrapper, None);
}

#[test]
fn multiline_empty_and_escaped_quotes() {
    let content = "[tasks.demo]\nempty_basic = \"\"\"\"\"\"\nempty_lit = ''''''\nquote = \"\"\"\"quoted\"\"\"\"\n";
    let wrapper = parse_cargo_wrapper(content).expect("parses");
    assert_eq!(wrapper, None);
}

#[test]
fn multiline_line_continuation() {
    let content = "[tasks.demo]\ntext = \"\"\"\nhello \\\n   world\"\"\"\n";
    let wrapper = parse_cargo_wrapper(content).expect("parses");
    assert_eq!(wrapper, None);
}

#[test]
fn multiline_with_cargo_wrapper() {
    let content = "[wrappers.cargo]\ncommand = \"\"\"\nmbx\"\"\"\n";
    let wrapper = parse_cargo_wrapper(content)
        .expect("parses")
        .expect("found");
    assert_eq!(wrapper.command, "mbx");
    assert_eq!(wrapper.line, 2);
}

#[test]
fn multiline_literal_with_cargo_wrapper() {
    let content = "[wrappers.cargo]\ncommand = '''\nmbx'''\n";
    let wrapper = parse_cargo_wrapper(content)
        .expect("parses")
        .expect("found");
    assert_eq!(wrapper.command, "mbx");
}

#[test]
fn nextest_array_of_tables_overrides() {
    let content = r"
[profile.default]
default-filter = 'not binary(/dind/)'

[[profile.default.overrides]]
filter = 'binary(/dind/)'
test-group = 'docker'

[[profile.default.overrides]]
filter = 'test(/agent/)'
test-group = 'agent'

[[profile.ci.overrides]]
filter = 'test(/agent/)'
test-group = 'agent'
";
    let config = parse_nextest_config(content).expect("parses array of tables");
    assert_eq!(config.profiles, vec!["ci".to_owned(), "default".to_owned()]);
    assert!(config.has_ci_profile());
    assert_eq!(config.selected_profile(), CI_PROFILE_NAME);
}

#[test]
fn duplicate_keys_within_array_table_rejected() {
    let content = r"
[[profile.default.overrides]]
filter = 'a'
filter = 'b'
";
    let err = parse_nextest_config(content).expect_err("duplicate in element must fail");
    assert_eq!(err.problem, "duplicate_key");
}

#[test]
fn table_redefined_as_array_table_rejected() {
    let content = r"
[profile.ci]
retries = 1

[[profile.ci]]
retries = 2
";
    let err = parse_nextest_config(content).expect_err("table to array table must fail");
    assert_eq!(err.problem, "duplicate_key");
}

#[test]
fn array_table_redefined_as_table_rejected() {
    let content = r"
[[profile.ci]]
retries = 1

[profile.ci]
retries = 2
";
    let err = parse_nextest_config(content).expect_err("array table to table must fail");
    assert_eq!(err.problem, "duplicate_key");
}

#[test]
fn multiline_key_rejected() {
    let content = "\"\"\"key\"\"\" = \"value\"\n";
    let err = parse_cargo_wrapper(content).expect_err("multiline key must fail");
    assert_eq!(err.problem, "multiline_key_unsupported");
}

#[test]
fn unterminated_multiline_string_rejected() {
    let content = "[tasks.demo]\nrun = '''\nunterminated\n";
    let err = parse_cargo_wrapper(content).expect_err("unterminated must fail");
    assert_eq!(err.problem, "unterminated_string");
}
