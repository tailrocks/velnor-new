//! Tofu isolation data cases: isolation pairs, per-root data dirs, M4 CLI config.
use std::ffi::OsString;
use velnor_actions_tofu::{
    TF_CLI_CONFIG_FILE_ENV, TF_DATA_DIR_ENV, TF_IN_AUTOMATION_ENV, TF_IN_AUTOMATION_ON,
    TF_INPUT_ENV, TF_INPUT_OFF, TF_PLUGIN_CACHE_DIR_ENV, tofu_cache_dir_under, tofu_cli_config,
    tofu_data_dir_under, tofu_isolation_env, tofu_root_slug,
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
fn root_slug_is_stable_case_folded_and_unique() {
    let root = tofu_root_slug("");
    assert!(root.starts_with("root-"), "{root}");
    assert_eq!(root, tofu_root_slug(""), "deterministic");
    let nested = tofu_root_slug("stacks/vpc");
    assert!(nested.starts_with("stacks-vpc-"), "{nested}");
    let (head, tag) = nested.rsplit_once('-').expect("digest suffix");
    assert_eq!(head, "stacks-vpc");
    assert_eq!(tag.len(), 12, "{nested}");
    assert!(tag.bytes().all(|b| b.is_ascii_hexdigit()), "{nested}");
    assert_ne!(tofu_root_slug("A"), tofu_root_slug("a"));
    assert!(tofu_root_slug("A").starts_with("a-"));
}

#[test]
fn cache_dir_mirrors_data_dir_under_its_base() {
    let base = "${{ runner.temp }}/velnor/tofu-cache";
    let root = tofu_cache_dir_under(base, "").expect("repo root maps");
    let data = tofu_data_dir_under("${{ runner.temp }}/velnor/tofu-data", "").expect("data maps");
    let cache_slug = root
        .strip_prefix(&format!("{base}/"))
        .expect("under cache base");
    let data_slug = data
        .strip_prefix("${{ runner.temp }}/velnor/tofu-data/")
        .expect("under data base");
    assert_eq!(cache_slug, data_slug, "same root shares one slug");
    assert!(root.starts_with(&format!("{base}/root-")), "{root}");
    let nested = tofu_cache_dir_under(base, "stacks/vpc").expect("nested maps");
    assert!(
        nested.starts_with(&format!("{base}/stacks-vpc-")),
        "{nested}"
    );
    assert!(tofu_cache_dir_under("", "stacks/vpc").is_err());
}

#[test]
fn data_dir_names_are_deterministic_and_unique() {
    let base = "${{ runner.temp }}/velnor/tofu-data";
    let root = tofu_data_dir_under(base, "").expect("repo root maps");
    let again = tofu_data_dir_under(base, "").expect("deterministic");
    assert_eq!(root, again);
    let (name, tag) = root.rsplit_once('-').expect("digest suffix");
    assert!(
        name.starts_with(&format!("{base}/root-")) || name == format!("{base}/root"),
        "{root}"
    );
    assert_eq!(tag.len(), 12, "{root}");
    assert!(tag.bytes().all(|b| b.is_ascii_hexdigit()), "{root}");
    let nested = tofu_data_dir_under(base, "stacks/vpc").expect("nested root maps");
    assert!(
        nested.starts_with(&format!("{base}/stacks-vpc-")),
        "{nested}"
    );
    assert_ne!(root, nested);
    let upper = tofu_data_dir_under(base, "A").expect("upper maps");
    let lower = tofu_data_dir_under(base, "a").expect("lower maps");
    assert_ne!(
        upper, lower,
        "slug case-folds but the digest must not collide"
    );
    assert!(upper.starts_with(&format!("{base}/a-")), "{upper}");
}

#[test]
fn data_dir_slug_truncates_but_digest_stays_unique() {
    let base = "/tmp/velnor/tofu-data";
    let long = "r".repeat(200);
    let first = tofu_data_dir_under(base, &long).expect("long root maps");
    let name = first.strip_prefix(&format!("{base}/")).expect("under base");
    let (slug, tag) = name.rsplit_once('-').expect("digest suffix");
    assert_eq!(slug.len(), 64, "{first}");
    assert_eq!(tag.len(), 12, "{first}");
    let other = format!("{}x", &long[..199]);
    let second = tofu_data_dir_under(base, &other).expect("sibling maps");
    assert_ne!(
        first, second,
        "truncated slugs share a prefix; digests must differ"
    );
}

#[test]
fn data_dir_rejects_empty_base() {
    let err = tofu_data_dir_under("", "stacks/vpc").expect_err("empty base must fail");
    assert!(err.to_string().contains("empty_base"), "got {err}");
}

#[test]
fn cli_config_is_plugin_cache_plus_checkpoint_only() {
    assert_eq!(
        tofu_cli_config("/tmp/velnor/cache").expect("config renders"),
        "plugin_cache_dir = \"/tmp/velnor/cache\"\ndisable_checkpoint = true\n"
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
        &"p".repeat(1025),
    ] {
        assert!(tofu_cli_config(bad).is_err(), "{bad:?} must fail closed");
    }
    assert!(
        tofu_cli_config(&"p".repeat(1024)).is_ok(),
        "1024 bytes pass"
    );
    let err = tofu_cli_config("/tmp/${var}").expect_err("interpolation must fail");
    assert!(err.to_string().contains("interpolation"), "got {err}");
}
