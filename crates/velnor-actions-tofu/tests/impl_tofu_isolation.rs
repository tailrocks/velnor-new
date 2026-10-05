//! Tofu isolation data cases: isolation pairs, per-root data dirs, M4 CLI config.
use std::ffi::OsString;
use velnor_actions_tofu::{
    TF_CLI_CONFIG_FILE_ENV, TF_DATA_DIR_ENV, TF_IN_AUTOMATION_ENV, TF_IN_AUTOMATION_ON,
    TF_INPUT_ENV, TF_INPUT_OFF, TF_PLUGIN_CACHE_DIR_ENV, tofu_cache_dir_under, tofu_cli_config,
    tofu_data_dir_under, tofu_isolation_env, tofu_root_locator,
};

#[test]
fn isolation_key_consts_are_exact() {
    assert_eq!(TF_DATA_DIR_ENV, "TF_DATA_DIR");
    assert_eq!(TF_PLUGIN_CACHE_DIR_ENV, "TF_PLUGIN_CACHE_DIR");
    assert_eq!(TF_CLI_CONFIG_FILE_ENV, "TF_CLI_CONFIG_FILE");
    assert_eq!(TF_IN_AUTOMATION_ENV, "TF_IN_AUTOMATION");
    assert_eq!(TF_INPUT_ENV, "TF_INPUT");
    assert_eq!(TF_IN_AUTOMATION_ON, "1");
    assert_eq!(TF_INPUT_OFF, "0");
}

#[test]
fn isolation_env_is_automation_pair_plus_paths() {
    assert_eq!(
        tofu_isolation_env(
            "/tmp/velnor/data",
            "/tmp/velnor/cli.hcl",
            "/tmp/velnor/cache"
        ),
        vec![
            (OsString::from("TF_IN_AUTOMATION"), OsString::from("1")),
            (OsString::from("TF_INPUT"), OsString::from("0")),
            (
                OsString::from("TF_DATA_DIR"),
                OsString::from("/tmp/velnor/data")
            ),
            (
                OsString::from("TF_CLI_CONFIG_FILE"),
                OsString::from("/tmp/velnor/cli.hcl")
            ),
            (
                OsString::from("TF_PLUGIN_CACHE_DIR"),
                OsString::from("/tmp/velnor/cache")
            ),
        ]
    );
}

#[test]
fn bounded_locators_and_dirs_preserve_exact_root_proof() {
    let roots = [
        "",
        "root",
        "A",
        "a",
        "a-b",
        "a_b",
        "a/b",
        "infra/日本語",
        "quote'/$`root",
    ];
    let mut registry = velnor_actions_tofu::RootLocatorRegistry::default();
    let paths: std::collections::BTreeSet<String> = roots
        .iter()
        .map(|root| registry.admit(root).expect("admit locator"))
        .collect();
    assert_eq!(paths.len(), roots.len());
    for root in roots {
        let path = tofu_root_locator(root).expect("locator");
        assert_eq!(path.len(), 67);
        assert_eq!(
            tofu_data_dir_under("/tmp/data", root).expect("data"),
            format!("/tmp/data/{path}")
        );
        assert_eq!(
            tofu_cache_dir_under("/tmp/cache", root).expect("cache"),
            format!("/tmp/cache/{path}")
        );
        assert_eq!(
            velnor_actions_tofu::root_for_key(&velnor_actions_tofu::key_for_root(root))
                .expect("exact proof"),
            root
        );
    }
}

#[test]
fn bounded_storage_supports_real_803_byte_source_root() {
    let temporary = crate::support::TempDir::create("long-root-storage").expect("temporary");
    let root = std::iter::repeat_n("r".repeat(200), 4)
        .collect::<Vec<_>>()
        .join("/");
    assert_eq!(root.len(), 803);
    std::fs::create_dir_all(temporary.path().join(&root)).expect("OS-valid source root");
    std::fs::write(
        temporary.path().join(&root).join("main.tf"),
        "variable \"x\" {}\n",
    )
    .expect("source file");
    let base = temporary.path().join("data");
    let path =
        tofu_data_dir_under(base.to_str().expect("UTF8 base"), &root).expect("bounded data path");
    std::fs::create_dir_all(&path).expect("OS-valid data path");
    let key = velnor_actions_tofu::key_for_root(&root);
    std::fs::write(std::path::Path::new(&path).join(".velnor-root-key"), &key)
        .expect("exact owner proof");
    assert_eq!(
        std::fs::read_to_string(std::path::Path::new(&path).join(".velnor-root-key"))
            .expect("proof"),
        key
    );
}

#[test]
fn long_multibyte_and_boundary_roots_do_not_expand_storage() {
    for root in [
        "r".repeat(64),
        "r".repeat(65),
        "r".repeat(125),
        "r".repeat(126),
        "r".repeat(128),
        "infra/".repeat(200).trim_end_matches('/').to_owned(),
        "日本語".repeat(80),
    ] {
        let locator = tofu_root_locator(&root).expect("long root");
        assert_eq!(locator.len(), 67);
        assert!(tofu_cli_config(&format!("/tmp/cache/{locator}")).is_ok());
    }
    for root in [".", "..", "/a", "a//b", "a\\b"] {
        assert!(tofu_root_locator(root).is_err(), "{root}");
    }
}

#[test]
fn data_dir_rejects_empty_base() {
    let err = tofu_data_dir_under("", "stacks/vpc").expect_err("empty base must fail");
    assert!(err.to_string().contains("empty_base"), "got {err}");
}

#[test]
fn cli_config_has_direct_installation_without_mirrors_or_overrides() {
    assert_eq!(
        tofu_cli_config("/tmp/velnor/cache").expect("config renders"),
        "plugin_cache_dir = \"/tmp/velnor/cache\"\ndisable_checkpoint = true\nprovider_installation {\n  direct {}\n}\n"
    );
}

#[test]
fn cli_config_rejects_injection_without_artificial_path_limit() {
    for bad in [
        "",
        "/tmp/has space/x\"quoted\"",
        "/tmp/back\\slash",
        "/tmp/new\nline",
        "/tmp/${var}",
        "/tmp/\t tab",
        "/tmp/\u{7f}del",
    ] {
        assert!(tofu_cli_config(bad).is_err(), "{bad:?} must fail closed");
    }
    assert!(
        tofu_cli_config(&"p".repeat(8192)).is_ok(),
        "path availability belongs to filesystem transport"
    );
    let err = tofu_cli_config("/tmp/${var}").expect_err("interpolation must fail");
    assert!(err.to_string().contains("interpolation"), "got {err}");
}
