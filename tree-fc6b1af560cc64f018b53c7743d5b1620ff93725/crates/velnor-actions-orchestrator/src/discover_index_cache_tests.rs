//! Pure byte-prefix cases for the reserved generated-cache root.

use super::is_reserved_cache_path_bytes;

#[test]
fn reserved_cache_prefix_matches_only_the_root_and_descendants() {
    for path in [
        b".velnor/cache".as_slice(),
        b".velnor/cache/cargo/registry/item.crate",
        b".velnor/cache/\xffpayload",
    ] {
        assert!(is_reserved_cache_path_bytes(path), "{path:?}");
    }
    for path in [
        b".velnor/cache-extra/item.crate".as_slice(),
        b".velnor/cache2/item.crate",
        b"src/generated/cache/item.crate",
        b".velnor/cachex/item.crate",
    ] {
        assert!(!is_reserved_cache_path_bytes(path), "{path:?}");
    }
}
