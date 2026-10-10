use crate::build_tasks::validate_config_shape;
use crate::native_tool_input::{NativeMiseConfig, NativeToolSource, native_mise_source};

const ROOT_CONFIG: &str = r#"
min_version = "2026.10.7"

[tools]
mr-boxington = "1.23.0"
rust = { version = "1.99.0", mr_boxington = true }

[settings]
lockfile = true
idiomatic_version_file_enable_tools = ["rust"]

[settings.cargo]
binstall = true

[tasks.native-ci]
run = "mbx +1.99.0 xtask desktop ci"
"#;

fn project(source: &str) -> NativeMiseConfig {
    let value = toml::from_str(source).expect("source TOML");
    let input = native_mise_source("mise.toml", source.as_bytes(), &value).expect("typed source");
    match input.source {
        NativeToolSource::MiseConfig(config) => config,
        NativeToolSource::RustToolchain | NativeToolSource::MiseLock(_) => panic!("wrong source"),
    }
}

#[test]
fn current_root_mise_shape_accepts_pinned_mbx_rust_without_wrappers() {
    let config = project(ROOT_CONFIG);
    validate_config_shape(&config, "1.99.0").expect("current pinned source");
    assert!(!config.root_keys.iter().any(|key| key == "wrappers"));
    assert_eq!(config.tools["rust"].mr_boxington, Some(true));
}

#[test]
fn legacy_wrapper_and_selected_tool_drift_fail_closed() {
    let legacy = format!("{ROOT_CONFIG}\n[wrappers.cargo]\ncommand = \"mbx\"\n");
    assert!(validate_config_shape(&project(&legacy), "1.99.0").is_err());

    let wrong_mbx = ROOT_CONFIG.replace("1.23.0", "1.22.0");
    assert!(validate_config_shape(&project(&wrong_mbx), "1.99.0").is_err());

    let wrong_rust = ROOT_CONFIG.replace("1.99.0", "1.98.0");
    assert!(validate_config_shape(&project(&wrong_rust), "1.98.0").is_err());

    let missing_route = ROOT_CONFIG.replace(", mr_boxington = true", "");
    assert!(validate_config_shape(&project(&missing_route), "1.99.0").is_err());

    let malformed_route = ROOT_CONFIG.replace("mr_boxington = true", "mr_boxington = \"true\"");
    assert!(validate_config_shape(&project(&malformed_route), "1.99.0").is_err());
}
