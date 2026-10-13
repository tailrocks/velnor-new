use super::Cli;

#[test]
fn portable_cli_version_matches_reported_source_identity() {
    assert_eq!(Cli::spec().version, Some(crate::version::VERSION));
}
