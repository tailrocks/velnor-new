//! Some parts of parsing block strings are deferred.
//!
//! This test file lets us exercise those code paths

use cynic_parser::{parse_executable_document, parse_type_system_document};

#[test]
fn should_read_sdl_short_block_string_description() {
    let sdl = r#"
        """Do"""
        type Query {
            hello: String
        }
    "#;

    let doc = parse_type_system_document(sdl).unwrap();
    assert_eq!(
        doc.definitions()
            .next()
            .unwrap()
            .as_type()
            .unwrap()
            .description()
            .unwrap()
            .to_cow(),
        "Do"
    );
}

#[test]
fn should_read_executable_short_block_string_description() {
    let sdl = r#"
        """Do"""
        query Query {
            hello
        }
    "#;

    let doc = parse_executable_document(sdl).unwrap();
    let description = doc
        .definitions()
        .next()
        .unwrap()
        .as_operation()
        .unwrap()
        .description()
        .unwrap();
    assert_eq!(description.literal().to_cow(), "Do");
}
