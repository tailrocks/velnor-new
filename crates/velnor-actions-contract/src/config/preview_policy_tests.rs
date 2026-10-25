use super::*;
use crate::ContractError;
use crate::{
    homebrew_spec::{
        HOMEBREW_FORMULA_SPEC_SCHEMA_V1, HOMEBREW_FORMULA_TEMPLATE_V1, HomebrewFormulaChannelSpec,
        HomebrewFormulaSpec, HomebrewHostProductSpec, HomebrewStableSentinel,
    },
    package_release::PackageReleaseChannel,
};

fn formula_spec(preview: bool, stable: bool) -> HomebrewFormulaSpec {
    let mut channels = Vec::new();
    if preview {
        channels.push(HomebrewFormulaChannelSpec {
            channel: PackageReleaseChannel::Preview,
            formula: "jackin-preview".to_owned(),
            formula_class: "JackinPreview".to_owned(),
            destination: "Formula/jackin-preview.rb".to_owned(),
            conflicts: vec!["jackin".to_owned()],
        });
    }
    if stable {
        channels.push(HomebrewFormulaChannelSpec {
            channel: PackageReleaseChannel::Stable,
            formula: "jackin".to_owned(),
            formula_class: "Jackin".to_owned(),
            destination: "Formula/jackin.rb".to_owned(),
            conflicts: vec!["jackin-preview".to_owned()],
        });
    }
    HomebrewFormulaSpec {
        schema: HOMEBREW_FORMULA_SPEC_SCHEMA_V1.to_owned(),
        template: HOMEBREW_FORMULA_TEMPLATE_V1.to_owned(),
        description: "Jackin command line tools".to_owned(),
        homepage: "https://github.com/jackin-project/jackin".to_owned(),
        license: "MIT".to_owned(),
        channels,
        host: HomebrewHostProductSpec {
            product: "jackin".to_owned(),
            targets: vec!["aarch64-apple-darwin".to_owned()],
            bins: vec!["jackin".to_owned()],
        },
        auxiliaries: Vec::new(),
        stable_sentinel: HomebrewStableSentinel {
            date: "2026-10-24".to_owned(),
            reason: "Stable release is not active".to_owned(),
        },
    }
}

fn config(stable: bool, preview: bool) -> VelnorConfig {
    let mut config: VelnorConfig = serde_json::from_value(serde_json::json!({
        "schema":1,
        "workflow":{"name":"CI","policy":"consumer-v1","generator_validation":"bootstrap","max_parallel_jobs":2},
        "resources":{"compiler_process_budget":2,"test_process_budget":2},
        "test_sharding":{"default_shards":1,"by_manifest":{}},
        "discovery":{"exclude":[]},
        "stacks":{"ignore":[],"rust":{"release":{"enabled":stable,"registry":"git-tag","packages":["jackin"],"tag_name":"v{{ version }}"},"binary_release":{
            "version_package":"jackin", "version_env":"JACKIN_VERSION_OVERRIDE",
            "evidence":{"profile":"signed-sbom-v1"},
            "producer":{"task":"release:package","tools":["rust"],"inputs":["mise.toml","Cargo.lock"]},
            "products":[{"id":"jackin","package":"jackin","bins":["jackin"],"targets":["aarch64-apple-darwin"],"archive":"tar.gz"}],
            "stable":{"enabled":stable},
            "preview":{"enabled":preview}
        }}}
    })).expect("typed config");
    config
        .qualified_tools
        .push(super::check_tool_reference_tests::declaration("rust"));
    config
}

