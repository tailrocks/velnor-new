use super::*;

#[test]
fn a_well_formed_record_is_accepted() {
    assert_eq!(verdict(&record()), "ACCEPTED");
}

#[test]
fn the_buildx_identity_must_be_a_version_with_an_optional_single_line_detail() {
    assert_eq!(
        buildx_version("github.com/docker/buildx v0.38.0"),
        Some("0.38.0")
    );
    assert_eq!(
        buildx_version("github.com/docker/buildx v0.38.0 abc"),
        Some("0.38.0")
    );
    for bad in [
        "github.com/docker/buildx v0.38",
        "github.com/docker/buildx v0.38.0 ",
        "github.com/docker/buildx v0.38.0 a\nb",
        "github.com/docker/buildx v0.38.0.1",
        "github.com/docker/buildx 0.38.0",
        "buildx v0.38.0",
    ] {
        assert_eq!(buildx_version(bad), None, "{bad:?}");
    }
}
