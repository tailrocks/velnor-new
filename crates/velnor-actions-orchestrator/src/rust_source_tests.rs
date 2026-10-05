use super::rust_source::extract_public_string_const;

fn extract(source: &str) -> Result<String, String> {
    extract_public_string_const(source, "PIN")
}

#[test]
fn reads_one_public_literal() {
    assert_eq!(
        extract("pub const PIN: &str = \"1.2.3\";"),
        Ok("1.2.3".to_owned())
    );
}

#[test]
fn ignores_line_and_nested_block_comment_decoys() {
    let source = concat!(
        "// pub const PIN: &str = \"old\";\n",
        "/* outer /* pub const PIN: &str = \"older\"; */ end */\n",
        "pub const PIN: &str = \"1.2.3\";\n",
    );
    assert_eq!(extract(source), Ok("1.2.3".to_owned()));
}

#[test]
fn string_contents_cannot_supply_a_declaration() {
    let source = r#"const NOTE: &str = "pub const PIN: &str = \"decoy\";";
pub const PIN: &str = "1.2.3";"#;
    assert_eq!(extract(source), Ok("1.2.3".to_owned()));
}

#[test]
fn raw_string_contents_cannot_supply_a_declaration() {
    let source = r####"const NOTE: &str = r###"pub const PIN: &str = "decoy";"###;
pub const PIN: &str = "1.2.3";"####;
    assert_eq!(extract(source), Ok("1.2.3".to_owned()));
}

#[test]
fn cooked_and_raw_literal_families_cannot_supply_declarations() {
    let source = r###"const BYTE = b"pub const PIN: &str = \"byte\";";
const RAW_BYTE = br#"pub const PIN: &str = "raw byte";"#;
const C = c"pub const PIN: &str = \"C string\";";
const RAW_C = cr##"pub const PIN: &str = "raw C string";"##;
const C_HEX = c"\xff";
pub const PIN: &str = "1.2.3";"###;
    assert_eq!(extract(source), Ok("1.2.3".to_owned()));
}

#[test]
fn macro_body_declaration_does_not_satisfy_pin() {
    let source = r#"macro_rules! fake { () => { pub const PIN: &str = "1.2.3"; } }"#;
    assert!(extract(source).is_err_and(|error| error.contains("missing const PIN")));
}

#[test]
fn macro_rules_all_delimiters_hide_body_declarations() {
    for (delimiter, closing) in [("(", ")"), ("[", "]"), ("{", "}")] {
        let source = format!(
            "macro_rules! fake {delimiter} () => {{ pub const PIN: &str = \"decoy\"; }} {closing}"
        );
        assert!(extract(&source).is_err_and(|error| error.contains("missing const PIN")));
    }
}

#[test]
fn raw_c_braces_inside_macro_body_do_not_end_the_group() {
    let source = concat!(
        "macro_rules! fake { () => { ",
        "const NOTE = cr##\"}} pub const PIN: &str = \"decoy\"; {{\"##; ",
        "pub const PIN: &str = \"macro decoy\"; }; }\n",
        "pub const PIN: &str = \"1.2.3\";\n",
    );
    assert_eq!(extract(source), Ok("1.2.3".to_owned()));
}

#[test]
fn nested_cfg_module_and_associated_const_are_not_authorities() {
    let nested = concat!(
        "#[cfg(feature = \"alternate\")]\n",
        "mod alternate { pub const PIN: &str = \"module decoy\"; }\n",
        "#[cfg_attr(feature = \"alternate\", cfg(feature = \"other\"))]\n",
        "mod alternate_again { pub const PIN: &str = \"cfg_attr decoy\"; }\n",
        "struct Holder;\n",
        "impl Holder { pub const PIN: &str = \"associated decoy\"; }\n",
    );
    assert!(extract(nested).is_err_and(|error| error.contains("missing const PIN")));
    assert_eq!(
        extract(&format!("{nested}pub const PIN: &str = \"1.2.3\";\n")),
        Ok("1.2.3".to_owned())
    );
}

#[test]
fn complete_byte_and_unicode_char_escapes_are_consumed() {
    let source = concat!(
        "const BYTE: u8 = b'\\x41';\n",
        "const CHAR: char = '\\u{1f642}';\n",
        "pub const PIN: &str = \"1.2.3\";\n",
    );
    assert_eq!(extract(source), Ok("1.2.3".to_owned()));
}

#[test]
fn non_ascii_hex_escape_remains_invalid_in_rust_string() {
    let source = r#"const NOTE = "\xff";
pub const PIN: &str = "1.2.3";"#;
    assert!(extract(source).is_err_and(|error| error.contains("hexadecimal escape")));
}

#[test]
fn commented_declaration_does_not_satisfy_pin() {
    let source = "// pub const PIN: &str = \"1.2.3\";\n";
    assert!(extract(source).is_err_and(|error| error.contains("missing const PIN")));
}

#[test]
fn duplicate_declarations_fail_closed() {
    let source = concat!(
        "pub const PIN: &str = \"1.2.3\";\n",
        "pub const PIN: &str = \"9.9.9\";\n",
    );
    assert!(extract(source).is_err_and(|error| error.contains("duplicate const PIN")));
}

#[test]
fn conditional_authority_fails_closed() {
    let source = concat!(
        "#[cfg(feature = \"alternate\")]\n",
        "pub const PIN: &str = \"1.2.3\";\n",
    );
    assert!(extract(source).is_err_and(|error| error.contains("unsupported authority attribute")));
}

#[test]
fn cfg_attr_target_and_conditional_crate_attrs_fail_closed() {
    let target = concat!(
        "#[cfg_attr(feature = \"alternate\", cfg(feature = \"other\"))]\n",
        "pub const PIN: &str = \"1.2.3\";\n",
    );
    assert!(extract(target).is_err_and(|error| error.contains("unsupported authority attribute")));

    for crate_attr in [
        "#![cfg(feature = \"alternate\")]",
        "#![cfg_attr(feature = \"alternate\", cfg(test))]",
        "#![r#cfg(any())]",
        "#![r#cfg_attr(feature = \"alternate\", r#cfg(test))]",
    ] {
        let source = format!("{crate_attr}\npub const PIN: &str = \"1.2.3\";\n");
        assert!(extract(&source).is_err_and(|error| error.contains("conditional crate attribute")));
    }
}

#[test]
fn escaped_pin_literal_fails_closed() {
    let source = r#"pub const PIN: &str = "1\x2e2.3";"#;
    assert!(extract(source).is_err_and(|error| error.contains("plain string literal")));
}
