//! Native recursive input-policy regression; official runner matching is qualified separately.
use super::{target_cache_inputs, target_cache_keys};
use velnor_actions_contract::discover::matches_glob;

fn admitted(patterns: &[String], path: &str) -> bool {
    let mut included = false;
    for pattern in patterns {
        let (negative, body) = pattern
            .strip_prefix('!')
            .map_or((false, pattern.as_str()), |body| (true, body));
        if matches_glob(body, path) {
            included = !negative;
        }
    }
    included
}

#[test]
fn root_inputs_cover_recursive_custom_members_and_build_scripts() {
    let (manifests, sources) = target_cache_inputs(None);
    for path in [
        "Cargo.toml",
        "crates/apps/cli/Cargo.toml",
        "components/deep/pkg/Cargo.toml",
        "members/target/Cargo.toml",
    ] {
        assert!(admitted(&manifests, path), "{path}");
        assert!(admitted(&sources, path), "{path}");
    }
    for path in [
        "build.rs",
        "crates/apps/cli/build.rs",
        "crates/ui/tui/src/lib.rs",
        "components/deep/pkg/tests/case.rs",
        "members/target/src/lib.rs",
    ] {
        assert!(!admitted(&manifests, path), "{path}");
        assert!(admitted(&sources, path), "{path}");
    }
    assert!(!admitted(&sources, "docs/notes.md"));
}

#[test]
fn only_actual_workspace_target_output_is_excluded() {
    for scope in [None, Some("components/workspace")] {
        let (manifests, sources) = target_cache_inputs(scope);
        let prefix = scope.map_or_else(String::new, |root| format!("{root}/"));
        for path in [
            "target/Cargo.toml",
            "target/debug/build/generated.rs",
            "target/velnor-clippy/debug/cache.rs",
        ] {
            let path = format!("{prefix}{path}");
            assert!(!admitted(&manifests, &path), "{path}");
            assert!(!admitted(&sources, &path), "{path}");
        }
        for path in ["members/target/Cargo.toml", "members/target/src/lib.rs"] {
            assert!(admitted(&sources, &format!("{prefix}{path}")), "{path}");
        }
    }
}

#[test]
fn nested_inputs_preserve_workspace_scope_and_root_toolchain_inputs() {
    let (manifests, sources) = target_cache_inputs(Some("components/workspace"));
    for path in [
        "components/workspace/custom/deep/Cargo.toml",
        "components/workspace/members/target/Cargo.toml",
    ] {
        assert!(admitted(&manifests, path), "{path}");
        assert!(admitted(&sources, path), "{path}");
    }
    assert!(admitted(
        &sources,
        "components/workspace/custom/deep/build.rs"
    ));
    for path in [
        "src/lib.rs",
        "other/workspace/src/lib.rs",
        "other/workspace/Cargo.toml",
    ] {
        assert!(!admitted(&sources, path), "{path}");
        assert!(!admitted(&manifests, path), "{path}");
    }
    assert!(admitted(&manifests, "rust-toolchain.toml"));
    assert!(!admitted(&sources, "rust-toolchain.toml"));
}

#[test]
fn omitted_empty_and_dot_root_scopes_have_identical_keys() {
    let root = target_cache_keys("generation", "1.99.1", None);
    assert_eq!(root, target_cache_keys("generation", "1.99.1", Some("")));
    assert_eq!(root, target_cache_keys("generation", "1.99.1", Some(".")));
}
