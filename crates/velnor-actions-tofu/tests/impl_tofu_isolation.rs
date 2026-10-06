//! Tofu isolation data cases: isolation pairs, per-root data dirs, M4 CLI config.
use std::ffi::OsString;
use velnor_actions_tofu::{
    MAX_CLI_CONFIG_PATH_BYTES, TF_CLI_CONFIG_FILE_ENV, TF_DATA_DIR_ENV, TF_IN_AUTOMATION_ENV,
    TF_IN_AUTOMATION_ON, TF_INPUT_ENV, TF_INPUT_OFF, TF_PLUGIN_CACHE_DIR_ENV, tofu_cache_dir_under,
    tofu_cli_config, tofu_data_dir_under, tofu_isolation_env, tofu_root_locator,
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
fn root_locators_are_fixed_length_deterministic_and_exact() {
    let roots = ["", "root", "A", "a", "a-b", "a_b", "a/b", "infra/日本語"];
    let mut registry = velnor_actions_tofu::RootLocatorRegistry::default();
    let locators: std::collections::BTreeSet<String> = roots
        .iter()
        .map(|root| registry.admit(root).expect("locator admitted"))
        .collect();
    assert_eq!(locators.len(), roots.len());
    for root in roots {
        let locator = tofu_root_locator(root).expect("locator");
        assert_eq!(locator.len(), 67);
        assert!(locator.starts_with("b3-"), "{locator}");
        assert_eq!(locator, tofu_root_locator(root).expect("deterministic"));
    }
    assert_ne!(
        tofu_root_locator("").expect("repo"),
        tofu_root_locator("root").expect("dir")
    );
}

#[test]
fn cache_dir_mirrors_data_dir_under_its_base() {
    let base = "${{ runner.temp }}/velnor/tofu-cache";
    let root = tofu_cache_dir_under(base, "").expect("repo root maps");
    let data = tofu_data_dir_under("${{ runner.temp }}/velnor/tofu-data", "").expect("data maps");
    let cache_locator = root
        .strip_prefix(&format!("{base}/"))
        .expect("under cache base");
    let data_locator = data
        .strip_prefix("${{ runner.temp }}/velnor/tofu-data/")
        .expect("under data base");
    assert_eq!(cache_locator, data_locator, "same root shares one locator");
    assert!(root.starts_with(&format!("{base}/b3-")), "{root}");
    let nested = tofu_cache_dir_under(base, "stacks/vpc").expect("nested maps");
    assert!(nested.starts_with(&format!("{base}/b3-")), "{nested}");
    assert!(tofu_cache_dir_under("", "stacks/vpc").is_err());
}

#[test]
fn data_dir_names_are_deterministic_and_unique() {
    let base = "${{ runner.temp }}/velnor/tofu-data";
    let root = tofu_data_dir_under(base, "").expect("repo root maps");
    let again = tofu_data_dir_under(base, "").expect("deterministic");
    assert_eq!(root, again);
    let locator = root.strip_prefix(&format!("{base}/")).expect("under base");
    assert_eq!(locator.len(), 67, "{root}");
    assert!(locator.starts_with("b3-"), "{root}");
    let nested = tofu_data_dir_under(base, "stacks/vpc").expect("nested root maps");
    assert!(nested.starts_with(&format!("{base}/b3-")), "{nested}");
    assert_ne!(root, nested);
    let upper = tofu_data_dir_under(base, "A").expect("upper maps");
    let lower = tofu_data_dir_under(base, "a").expect("lower maps");
    assert_ne!(upper, lower, "locators preserve exact source root bytes");
    assert!(upper.starts_with(&format!("{base}/b3-")), "{upper}");
}

#[test]
fn data_dir_locator_stays_bounded_for_long_roots() {
    let base = "/tmp/velnor/tofu-data";
    let long = "r".repeat(200);
    let first = tofu_data_dir_under(base, &long).expect("long root maps");
    let name = first.strip_prefix(&format!("{base}/")).expect("under base");
    assert_eq!(name.len(), 67, "{first}");
    let other = format!("{}x", &long[..199]);
    let second = tofu_data_dir_under(base, &other).expect("sibling maps");
    assert_ne!(first, second, "different roots must not share locators");
    assert!(tofu_data_dir_under(base, "../escape").is_err());
}

#[test]
fn data_dir_rejects_empty_base() {
    let err = tofu_data_dir_under("", "stacks/vpc").expect_err("empty base must fail");
    assert!(err.to_string().contains("empty_base"), "got {err}");
}

#[test]
fn cli_config_uses_direct_provider_installation() {
    assert_eq!(
        tofu_cli_config("/tmp/velnor/cache").expect("config renders"),
        "plugin_cache_dir = \"/tmp/velnor/cache\"\ndisable_checkpoint = true\nprovider_installation {\n  direct {}\n}\n"
    );
}

#[test]
fn cli_config_rejects_injection_and_oversize() {
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
    let at_limit = format!("/{}", "p".repeat(MAX_CLI_CONFIG_PATH_BYTES - 1));
    assert!(tofu_cli_config(&at_limit).is_ok(), "1024 bytes pass");
    let over_limit = format!("/{}", "p".repeat(MAX_CLI_CONFIG_PATH_BYTES));
    let err = tofu_cli_config(&over_limit).expect_err("oversize path must fail");
    assert!(err.to_string().contains("oversize_cache_dir"), "{err}");
    let err = tofu_cli_config("/tmp/${var}").expect_err("interpolation must fail");
    assert!(err.to_string().contains("interpolation"), "got {err}");
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
