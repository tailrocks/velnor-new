use super::OwnedConfig;

#[test]
fn native_protocol_preserves_order_duplicates_empty_and_multiline_values() {
    let bytes = b"core.autocrlf\nfalse\0diff.driver.wordregex\na\nb\0core.autocrlf\n\0";
    let config = OwnedConfig::from_native_output(bytes).expect("native tuples");
    assert!(config.matches_native_output(bytes));
    assert!(!config.matches_native_output(
        b"core.autocrlf\n\0diff.driver.wordregex\na\nb\0core.autocrlf\nfalse\0"
    ));
}

#[test]
fn resolved_includes_and_owned_routing_do_not_reopen_source_config() {
    let config = OwnedConfig::from_native_output(
        b"include.path\nsecret-source\0includeif.gitdir:/x/.path\nother\0extensions.worktreeconfig\ntrue\0core.bare\nfalse\0core.eol\nlf\0",
    )
    .expect("resolved routing");
    assert!(config.matches_native_output(b"core.bare\nfalse\0core.eol\nlf\0"));
}

#[test]
fn delegates_promisors_submodules_and_unknown_extensions_are_refused() {
    for bytes in [
        b"filter.driver.clean\nsecret-command\0".as_slice(),
        b"filter.driver.process\n\0".as_slice(),
        b"remote.origin.promisor\ntrue\0".as_slice(),
        b"extensions.refstorage\nreftable\0".as_slice(),
        b"submodule.name.path\nsub\0".as_slice(),
        b"diff.submodule\ndiff\0".as_slice(),
    ] {
        let Err(error) = OwnedConfig::from_native_output(bytes) else {
            unreachable!("unsupported context admitted");
        };
        assert!(!error.to_string().contains("secret-command"));
    }
}

#[test]
fn malformed_or_valueless_native_records_are_refused() {
    for bytes in [
        b"key".as_slice(),
        b"key\0".as_slice(),
        b"\nvalue\0".as_slice(),
    ] {
        assert!(OwnedConfig::from_native_output(bytes).is_err());
    }
    let config = OwnedConfig::from_native_output(b"").expect("empty configuration");
    assert!(config.matches_native_output(b""));
}
