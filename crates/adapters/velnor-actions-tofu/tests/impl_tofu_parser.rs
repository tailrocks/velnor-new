//! Bounded structural parser cases (S8).
use velnor_actions_tofu_core::parser::{
    MAX_DEPTH, ParseError, has_legacy_ref_text, parse_json, parse_native, strip_template_spans,
};

#[test]
fn empty_native_parses_to_empty_model() {
    let model = parse_native("").expect("empty parses");
    assert!(model.blocks.is_empty());
    assert!(model.required_versions.is_empty());
    assert!(!model.has_legacy_ref);
}

#[test]
fn native_blocks_carry_kinds_and_labels() {
    let model = parse_native(
        "variable \"x\" {}\nresource \"aws_a\" \"b\" {}\nmodule \"m\" {}\nterraform {}\n",
    )
    .expect("blocks parse");
    let kinds: Vec<(&str, Vec<&str>)> = model
        .blocks
        .iter()
        .map(|block| {
            (
                block.kind.as_str(),
                block.labels.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("variable", vec!["x"]),
            ("resource", vec!["aws_a", "b"]),
            ("module", vec!["m"]),
            ("terraform", vec![]),
        ]
    );
}

#[test]
fn native_required_version_literals_collect() {
    let model = parse_native(
        "terraform {\n  required_version = \">= 1.6\"\n}\nterraform {\n  required_version = \"< 2.0\"\n}\n",
    )
    .expect("versions parse");
    assert_eq!(model.required_versions, vec![">= 1.6", "< 2.0"]);
    assert!(!model.has_legacy_ref);
}

#[test]
fn native_keywords_and_labels_never_fire_legacy() {
    for source in [
        "terraform {\n}\n",
        "terraform {\n  required_version = \">= 1.6\"\n}\n",
        "resource \"terraform_data\" \"x\" {}\n",
        "data \"terraform_remote_state\" \"x\" {}\n",
        "variable \"terraform\" {}\n",
        "output \"v\" {\n  value = terraform.workspace\n}\n",
    ] {
        let model = parse_native(source).expect("parses");
        assert!(!model.has_legacy_ref, "{source:?}");
    }
}

#[test]
fn native_string_literals_fire_legacy() {
    let model =
        parse_native("resource \"t\" \"n\" {\n  cmd = \"terraform plan\"\n}\n").expect("parses");
    assert!(model.has_legacy_ref);
    let model =
        parse_native("locals {\n  a = [\"x\", \"run terraform apply\"]\n}\n").expect("parses");
    assert!(model.has_legacy_ref);
}

#[test]
fn template_interpolations_strip_but_literals_scan() {
    assert_eq!(
        strip_template_spans("prefix-${var.x}-suffix"),
        "prefix--suffix"
    );
    assert_eq!(strip_template_spans("%{ if x }a%{ endif }"), "a");
    assert_eq!(strip_template_spans("plain"), "plain");
    let model =
        parse_native("output \"v\" {\n  value = \"${terraform.workspace}\"\n}\n").expect("parses");
    assert!(!model.has_legacy_ref);
    let model =
        parse_native("output \"v\" {\n  value = \"run terraform ${var.x}\"\n}\n").expect("parses");
    assert!(model.has_legacy_ref);
    // Template-joined tokens stay silent: the literal ends at `-`,
    // so the boundary check (correctly for suffixed names) misses.
    let model =
        parse_native("output \"v\" {\n  value = \"run terraform-${var.x}\"\n}\n").expect("parses");
    assert!(!model.has_legacy_ref);
}

#[test]
fn legacy_token_needs_standalone_boundaries() {
    assert!(has_legacy_ref_text("terraform plan"));
    assert!(has_legacy_ref_text("run:terraform!"));
    assert!(!has_legacy_ref_text("registry.terraform.io/x"));
    assert!(!has_legacy_ref_text("myterraform"));
    assert!(!has_legacy_ref_text("terraform_data"));
    assert!(!has_legacy_ref_text("terraform-foo"));
    assert!(!has_legacy_ref_text("Terraform"));
}

#[test]
fn native_syntax_errors_are_single_line() {
    let err = parse_native("variable \"x\" {").expect_err("unclosed fails");
    assert!(matches!(err, ParseError::Syntax { .. }));
    assert!(!err.to_string().contains('\n'), "{err}");
}

#[test]
fn deep_nesting_trips_the_depth_budget() {
    use std::fmt::Write as _;
    let mut source = String::new();
    for index in 0..MAX_DEPTH + 8 {
        writeln!(source, "resource \"t{index}\" \"n\" {{").expect("buffer");
    }
    for _ in 0..MAX_DEPTH + 8 {
        source.push_str("}\n");
    }
    let err = parse_native(&source).expect_err("too deep fails");
    assert_eq!(err, ParseError::TooDeep);
}

#[test]
fn oversize_input_trips_the_byte_cap() {
    let big = " ".repeat(1_048_577);
    let err = parse_native(&big).expect_err("oversize fails");
    assert!(matches!(err, ParseError::TooLarge { .. }), "{err}");
}

#[test]
fn json_root_must_be_an_object() {
    let err = parse_json("[1, 2]").expect_err("array root fails");
    assert_eq!(err, ParseError::NotObject);
    let err = parse_json("\"x\"").expect_err("scalar root fails");
    assert_eq!(err, ParseError::NotObject);
}

#[test]
fn json_blocks_share_the_identity_space() {
    let model = parse_json(
        "{\"variable\": {\"x\": {}}, \"resource\": {\"aws_a\": {\"b\": {}}}, \"terraform\": {\"required_version\": \"= 1.5.7\"}}",
    )
    .expect("json parses");
    let kinds: Vec<(&str, Vec<&str>)> = model
        .blocks
        .iter()
        .map(|block| {
            (
                block.kind.as_str(),
                block.labels.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert!(kinds.contains(&("variable", vec!["x"])));
    assert!(kinds.contains(&("resource", vec!["aws_a", "b"])));
    assert_eq!(model.required_versions, vec!["= 1.5.7"]);
}

#[test]
fn json_misshapen_tracked_kinds_error() {
    let err = parse_json("{\"variable\": 42}").expect_err("scalar variable fails");
    assert!(matches!(err, ParseError::Syntax { .. }), "{err}");
    let err = parse_json("{\"resource\": {\"t\": 42}}").expect_err("scalar names fails");
    assert!(matches!(err, ParseError::Syntax { .. }), "{err}");
}

#[test]
fn json_strings_scan_for_legacy_refs() {
    let model = parse_json("{\"locals\": {\"cmd\": \"terraform apply\"}}").expect("parses");
    assert!(model.has_legacy_ref);
    let model = parse_json("{\"terraform\": {\"required_version\": \">= 1.0\"}}").expect("parses");
    assert!(!model.has_legacy_ref);
}
