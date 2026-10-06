use super::check_source_graph;
fn violations(source: &str, fixture: &str) -> Vec<String> {
    let inputs = vec![
        (
            "crates/velnor-actions-mise/src/lib.rs".to_owned(),
            source.to_owned(),
        ),
        (
            "crates/velnor-actions-mise/tests/fixture.rs".to_owned(),
            fixture.to_owned(),
        ),
    ];
    let mut found = Vec::new();
    check_source_graph(&inputs, &mut found);
    found
}
#[test]
fn test_module_role_requires_every_literal_incoming_edge() {
    let valid = "#[cfg(test)] #[path=\"../tests/fixture.rs\"] mod fixture;";
    assert!(violations(valid, "fn setup(){ std::fs::write(p,b\"x\"); }").is_empty());
    for source in [
        "#[path=\"../tests/fixture.rs\"] mod fixture;",
        "#[cfg(any(test,feature=\"extra\"))] #[path=\"../tests/fixture.rs\"] mod fixture;",
        "#[cfg_attr(test,path=\"../tests/fixture.rs\")] mod fixture;",
        "#[cfg(unknown)] #[path=\"../tests/fixture.rs\"] mod fixture;",
        "#[cfg(test)] #[path=concat!(\"../tests/\",\"fixture.rs\")] mod fixture;",
        "#[cfg(test)] #[path=\"../../outside.rs\"] mod fixture;",
        "#[cfg(test)] #[path=\"../tests/fixture.rs\"] pub mod fixture;",
        "#[cfg(test)] #[path=\"../tests/fixture.rs\"] mod fixture; pub use fixture::*;",
        "#[cfg(test)] #[path=\"../tests/fixture.rs\"] mod fixture; #[path=\"../tests/fixture.rs\"] mod second;",
    ] {
        assert!(!violations(source, "").is_empty(), "{source}");
    }
}
#[test]
fn mixed_inline_product_effects_and_opaque_test_expansion_are_rejected() {
    let good = "#[cfg(test)] mod tests { fn setup(){std::fs::write(p,b\"x\");} }";
    assert!(violations(good, "").is_empty());
    assert!(
        !violations(
            &format!("{good} fn rogue(){{std::fs::write(p,b\"x\");}}"),
            ""
        )
        .is_empty()
    );
    for fixture in [
        "include!(\"writer.inc\");",
        "writer!();",
        "env!(\"WRITER\");",
        "concat!(\"writer\");",
        "use external::assert; assert!(\"writer\");",
    ] {
        assert!(
            !violations(
                "#[cfg(test)] #[path=\"../tests/fixture.rs\"] mod fixture;",
                fixture
            )
            .is_empty()
        );
    }
}

#[test]
fn recursive_shared_fixture_edges_and_cycles_are_closed() {
    let mut units = vec![
        ("crates/velnor-actions-mise/src/lib.rs".to_owned(), "#[cfg(test)] #[path=\"../tests/fixture.rs\"] mod fixture;".to_owned()),
        ("crates/velnor-actions-mise/tests/fixture.rs".to_owned(), "#[cfg(test)] #[path=\"../../test_support/git_fixture.rs\"] pub(crate) mod git_fixture; pub(crate) use git_fixture::setup;".to_owned()),
        ("crates/test_support/git_fixture.rs".to_owned(), "pub(crate) fn setup(){std::fs::write(p,b\"x\");}".to_owned()),
    ];
    let mut found = Vec::new();
    check_source_graph(&units, &mut found);
    assert!(found.is_empty(), "{found:?}");
    units[1].1 = units[1].1.replace("#[cfg(test)]", "");
    check_source_graph(&units, &mut found);
    assert!(!found.is_empty());
    assert!(
        !violations(
            "#[cfg(test)] #[path=\"lib.rs\"] mod cycle; fn rogue(){std::fs::write(p,b\"x\");}",
            ""
        )
        .is_empty()
    );
    assert!(
        !violations(
            "mod outer { #[cfg(test)] mod hidden { fn write(){} } } pub use outer::hidden::*;",
            ""
        )
        .is_empty()
    );
}

#[test]
fn product_type_trait_and_pattern_paths_cannot_export_test_authority() {
    let prefix = "#[cfg(test)] #[path=\"../tests/fixture.rs\"] mod fixture;";
    for source in [
        "#[cfg(test)] pub type Escape = fixture::Writer;",
        "pub struct Escape { pub writer: fixture::Writer }",
        "pub fn escape<T:fixture::Trait>() {}",
        "impl fixture::Trait for Product {}",
        "fn escape(v:Value) { match v { fixture::Writer {..} => {} } }",
        "pub use fixture::Writer as Escape;",
    ] {
        assert!(
            !violations(
                &format!("{prefix}{source}"),
                "pub struct Writer; pub trait Trait {}"
            )
            .is_empty(),
            "{source}"
        );
    }
}

#[test]
fn local_module_declarations_cannot_escape_graph_inventory() {
    for source in [
        "fn rogue(){ #[cfg(test)] #[path=\"../tests/unknown.rs\"] mod hidden; }",
        "#[cfg(test)] mod tests { fn rogue(){ #[cfg(test)] #[path=\"../outside.rs\"] mod hidden; } }",
        "const HIDDEN:() = { #[cfg(test)] #[path=\"../tests/unknown.rs\"] mod hidden; };",
    ] {
        assert!(!violations(source, "").is_empty(), "{source}");
    }
}

#[test]
fn macro_code_paths_cannot_export_test_authority_but_literals_stay_data() {
    let prefix = "#[cfg(test)] #[path=\"../tests/fixture.rs\"] mod fixture;";
    for body in [
        "assert!(fixture::predicate());",
        "format!(\"{}\",fixture::Writer);",
        "vec![fixture::Writer; 2];",
        "matches!(value,fixture::Writer {..});",
        "matches!(value,r#fixture::Writer {..});",
        "vec![std::fs::r#write(p,b\"x\"); 2];",
        "assert!({use fixture as alias; alias::predicate()});",
        "serde_json::json!({\"key\":{use {fixture as alias};alias::predicate()}});",
    ] {
        let source = format!("{prefix} #[cfg(test)] pub fn exposed(){{{body}}}");
        assert!(
            !violations(&source, "pub struct Writer;").is_empty(),
            "{body}"
        );
    }
    assert!(
        violations(
            &format!(
                "{prefix} fn data(){{format!(\"fixture::Writer\"); stringify!(fixture::Writer);}}"
            ),
            "pub struct Writer;"
        )
        .is_empty()
    );
}
