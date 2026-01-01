use super::*;

/// Slug shapes: canonical and cased slugs normalize, everything
/// else is not a slug.
#[test]
fn repository_slug_shapes() {
    assert_eq!(validate_repository_slug("o/r").as_deref(), Some("o/r"));
    assert_eq!(validate_repository_slug("O/R").as_deref(), Some("o/r"));
    assert_eq!(
        validate_repository_slug("tailrocks/velnor-new").as_deref(),
        Some("tailrocks/velnor-new")
    );
    for raw in [
        "",
        "only-owner",
        "o/r/extra",
        "/r",
        "o/",
        "/",
        "o/r ",
        " o/r",
        "o /r",
        "o/r\n",
    ] {
        assert!(
            validate_repository_slug(raw).is_none(),
            "{raw:?} must not validate"
        );
    }
}
