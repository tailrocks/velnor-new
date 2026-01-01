//! Docs-lane input tests: dirs, base path, and smoke routes.
use super::*;

#[test]
fn reference_defaults_pass() {
    let lane = DocsLaneConfig::defaults_for("/docs");
    assert!(lane.validate("test").is_ok());
    assert_eq!(lane.smoke_routes, vec!["/".to_owned(), "/docs".to_owned()]);
}

#[test]
fn dirs_reject_absolute_traversal_and_empty() {
    let mut lane = DocsLaneConfig::defaults_for("/docs");
    for bad in [
        "",
        "/docs",
        "docs/../etc",
        "docs//x",
        "do\\cs",
        "docs/x..y/../z",
    ] {
        lane.app_dir = bad.to_owned();
        assert!(lane.validate("test").is_err(), "{bad}");
        lane.app_dir = "docs".to_owned();
        lane.content_dir = bad.to_owned();
        assert!(lane.validate("test").is_err(), "{bad}");
        lane.content_dir = "content/docs".to_owned();
        lane.output_dir = bad.to_owned();
        assert!(lane.validate("test").is_err(), "{bad}");
        lane.output_dir = ".output/public".to_owned();
    }
    lane.output_dir = ".output/public".to_owned();
    assert!(lane.validate("test").is_ok());
}

#[test]
fn base_path_needs_absolute_segment_without_trailing_slash() {
    let mut lane = DocsLaneConfig::defaults_for("/docs");
    for bad in ["", "/", "docs", "/docs/", "/docs/../x", "/do cs"] {
        lane.base_path = bad.to_owned();
        let err = lane.validate("test").expect_err("base must fail");
        assert!(err.to_string().contains("malformed_base_path"), "{err}");
    }
    lane.base_path = "/docs".to_owned();
    assert!(lane.validate("test").is_ok());
}

#[test]
fn smoke_routes_must_be_sorted_unique_absolute() {
    let mut lane = DocsLaneConfig::defaults_for("/docs");
    lane.smoke_routes = Vec::new();
    assert!(lane.validate("test").is_err());
    lane.smoke_routes = vec!["/docs".to_owned(), "/".to_owned()];
    let err = lane.validate("test").expect_err("order must fail");
    assert!(err.to_string().contains("must_be_sorted"), "{err}");
    lane.smoke_routes = vec!["/".to_owned(), "/".to_owned()];
    let err = lane.validate("test").expect_err("dup must fail");
    assert!(err.to_string().contains("duplicate_route"), "{err}");
    lane.smoke_routes = vec!["/".to_owned(), "docs".to_owned()];
    let err = lane.validate("test").expect_err("relative must fail");
    assert!(err.to_string().contains("malformed_route"), "{err}");
    lane.smoke_routes = vec!["/".to_owned(), "/docs/../x".to_owned()];
    assert!(lane.validate("test").is_err());
}
