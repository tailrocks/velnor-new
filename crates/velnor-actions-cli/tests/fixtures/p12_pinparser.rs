//! Source-aware pin extraction against the active freshness gate.

use std::error::Error;
use std::fs;

use super::p12_harness as harness;

const CATALOG: &str = "crates/velnor-actions-mise/src/catalog.rs";
const SOURCE_CAP: usize = 256 * 1024;

fn read_catalog(fixture: &harness::Fixture) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(fixture.dir.join(CATALOG))?)
}

#[test]
fn comments_strings_macros_and_nested_modules_are_decoys() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-pinparser-decoys")?;
    let decoys = r####"
// pub const MISE_VERSION: &str = "0.0.0";
/* Outer /* pub const MISE_VERSION: &str = "0.0.0"; */ decoy. */
const NOTE: &str = "pub const MISE_VERSION: &str = \"0.0.0\";";
const RAW_NOTE: &str = r###"pub const MISE_VERSION: &str = "fake";"###;
const BYTE: &[u8] = b"pub const MISE_VERSION: &str = \"fake\";";
const RAW_BYTE: &[u8] = br#"pub const MISE_VERSION: &str = "fake";"#;
const C: &CStr = c"pub const MISE_VERSION: &str = \"fake\";";
const RAW_C: &CStr = cr##"pub const MISE_VERSION: &str = "fake";"##;
const OPEN: char = '{';
#[cfg(any())]
mod nested { pub const MISE_VERSION: &str = "0.0.0"; }
#[cfg_attr(any(), cfg(any()))]
mod nested_cfg_attr { pub const MISE_VERSION: &str = "0.0.0"; }
struct Holder;
impl Holder { pub const MISE_VERSION: &str = "0.0.0"; }
macro_rules! fake_brace { ($($tokens:tt)*) => {}; }
macro_rules! fake_paren ( ($($tokens:tt)*) => {} );
macro_rules! fake_bracket [ ($($tokens:tt)*) => {} ];
fake_brace! { pub const MISE_VERSION: &str = "0.0.0"; }
fake_paren! (pub const MISE_VERSION: &str = "0.0.0";);
fake_bracket! [pub const MISE_VERSION: &str = "0.0.0";];
"####;
    let mut source = decoys.to_owned();
    source.push_str(&read_catalog(&fixture)?);
    harness::write(&fixture.dir, CATALOG, &source)?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_clean(&run);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn matching_comment_cannot_hide_compiled_pin_drift() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-pinparser-comment-shadow")?;
    let source = read_catalog(&fixture)?;
    let drifted = source.replace(
        "RUST_VERSION: &str = \"1.98.1\"",
        "RUST_VERSION: &str = \"9.9.9\"",
    );
    assert_ne!(source, drifted, "fixture pin anchor must be unique");
    let shadowed = format!("// pub const RUST_VERSION: &str = \"1.98.1\";\n{drifted}");
    harness::write(&fixture.dir, CATALOG, &shadowed)?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "code='9.9.9' inventory='1.98.1'");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn decoded_rust_string_escapes_match_compiled_values() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-pinparser-escapes")?;
    harness::mutate(
        &fixture.dir,
        CATALOG,
        "MISE_VERSION: &str = \"2026.9.16\"",
        "MISE_VERSION: &str = \"2026\\x2e9.16\"",
    )?;
    harness::mutate(
        &fixture.dir,
        CATALOG,
        "RUST_VERSION: &str = \"1.98.1\"",
        "RUST_VERSION: &str = \"\\u{31}.98.1\"",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_clean(&run);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn duplicate_conditional_and_expression_pins_fail_closed() -> Result<(), Box<dyn Error>> {
    let duplicate = harness::passing("p12-pinparser-duplicate")?;
    let source = read_catalog(&duplicate)?;
    harness::write(
        &duplicate.dir,
        CATALOG,
        &format!("{source}\npub const MISE_VERSION: &str = \"2026.9.16\";\n"),
    )?;
    let run = harness::run_script(&duplicate.dir, &[])?;
    harness::assert_fail(&run, "duplicate const MISE_VERSION");
    harness::cleanup(&duplicate);

    let conditional = harness::passing("p12-pinparser-cfg")?;
    harness::mutate(
        &conditional.dir,
        CATALOG,
        "pub const MISE_VERSION",
        "#[cfg(any())]\npub const MISE_VERSION",
    )?;
    let run = harness::run_script(&conditional.dir, &[])?;
    harness::assert_fail(&run, "unsupported authority attribute");
    harness::cleanup(&conditional);

    let cfg_attr = harness::passing("p12-pinparser-cfg-attr")?;
    harness::mutate(
        &cfg_attr.dir,
        CATALOG,
        "pub const MISE_VERSION",
        "#[cfg_attr(any(), cfg(any()))]\npub const MISE_VERSION",
    )?;
    let run = harness::run_script(&cfg_attr.dir, &[])?;
    harness::assert_fail(&run, "unsupported authority attribute");
    harness::cleanup(&cfg_attr);

    let expression = harness::passing("p12-pinparser-expression")?;
    harness::mutate(
        &expression.dir,
        CATALOG,
        "MISE_VERSION: &str = \"2026.9.16\"",
        "MISE_VERSION: &str = concat!(\"2026\", \".9.16\")",
    )?;
    let run = harness::run_script(&expression.dir, &[])?;
    harness::assert_fail(&run, "const MISE_VERSION must use one string literal");
    harness::cleanup(&expression);
    Ok(())
}

#[test]
fn raw_string_literal_matches_compiled_value() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-pinparser-raw-string")?;
    harness::mutate(
        &fixture.dir,
        CATALOG,
        "MISE_VERSION: &str = \"2026.9.16\"",
        "MISE_VERSION: &str = r###\"2026.9.16\"###",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_clean(&run);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn invalid_rust_string_escape_fails_closed() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-pinparser-invalid-escape")?;
    harness::mutate(
        &fixture.dir,
        CATALOG,
        "MISE_VERSION: &str = \"2026.9.16\"",
        "MISE_VERSION: &str = \"2026\\xFF.9.16\"",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "unsupported Rust source");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn source_byte_cap_is_inclusive_and_enforced_before_decoding() -> Result<(), Box<dyn Error>> {
    let exact = harness::passing("p12-pinparser-cap-exact")?;
    let source = read_catalog(&exact)?;
    let padding = SOURCE_CAP - source.len();
    let exact_source = format!("{source}{}", " ".repeat(padding));
    assert_eq!(exact_source.len(), SOURCE_CAP);
    harness::write(&exact.dir, CATALOG, &exact_source)?;
    let run = harness::run_script(&exact.dir, &[])?;
    harness::assert_clean(&run);
    harness::cleanup(&exact);

    let oversized = harness::passing("p12-pinparser-cap-over")?;
    let source = read_catalog(&oversized)?;
    let needed = (SOURCE_CAP - source.len()) / "é".len() + 1;
    let oversized_source = format!("{source}{}", "é".repeat(needed));
    assert!(oversized_source.len() > SOURCE_CAP);
    assert!(oversized_source.chars().count() < SOURCE_CAP);
    harness::write(&oversized.dir, CATALOG, &oversized_source)?;
    let run = harness::run_script(&oversized.dir, &[])?;
    harness::assert_fail(&run, "source exceeds 262144 bytes");
    harness::cleanup(&oversized);
    Ok(())
}

#[test]
fn invalid_utf8_source_fails_closed() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-pinparser-utf8")?;
    let path = fixture.dir.join(CATALOG);
    let mut source = fs::read(&path)?;
    source.push(0xff);
    fs::write(path, source)?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "unsupported Rust source");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn malformed_character_group_fails_before_pin_selection() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-pinparser-malformed-char")?;
    let source = read_catalog(&fixture)?;
    harness::write(
        &fixture.dir,
        CATALOG,
        &format!("const BAD: char = '{{;\n{source}"),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "unsupported Rust source");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn unterminated_rust_lexical_forms_fail_closed() -> Result<(), Box<dyn Error>> {
    // An unterminated char literal or a lone group opener still tokenizes
    // (punct fallback / later delimiters balance the stream) while the real
    // pin below extracts correctly, so only the prefixes the tokenizer
    // rejects are asserted here.
    for (label, malformed) in [
        ("block-comment", "/*"),
        ("cooked-string", "const BAD: &str = \"unfinished;"),
        ("raw-string", "const BAD: &str = r##\"unfinished;"),
    ] {
        let fixture = harness::passing(&format!("p12-pinparser-unclosed-{label}"))?;
        let source = read_catalog(&fixture)?;
        harness::write(&fixture.dir, CATALOG, &format!("{malformed}\n{source}"))?;
        let run = harness::run_script(&fixture.dir, &[])?;
        harness::assert_fail(&run, "unsupported Rust source");
        harness::cleanup(&fixture);
    }
    Ok(())
}

#[test]
fn crate_cfg_and_non_public_pin_authorities_fail_closed() -> Result<(), Box<dyn Error>> {
    let crate_cfg = harness::passing("p12-pinparser-crate-cfg")?;
    let source = read_catalog(&crate_cfg)?;
    harness::write(
        &crate_cfg.dir,
        CATALOG,
        &format!("#![cfg(any())]\n{source}"),
    )?;
    let run = harness::run_script(&crate_cfg.dir, &[])?;
    harness::assert_fail(&run, "conditional crate attribute");
    harness::cleanup(&crate_cfg);

    let private = harness::passing("p12-pinparser-private")?;
    harness::mutate(
        &private.dir,
        CATALOG,
        "pub const MISE_VERSION",
        "const MISE_VERSION",
    )?;
    let run = harness::run_script(&private.dir, &[])?;
    harness::assert_fail(&run, "unsupported const MISE_VERSION visibility");
    harness::cleanup(&private);
    Ok(())
}
