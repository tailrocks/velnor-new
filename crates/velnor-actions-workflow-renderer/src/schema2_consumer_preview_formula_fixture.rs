pub(super) fn formula_spec_fixture() -> velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec
{
    use velnor_actions_contract::homebrew_spec::{
        HOMEBREW_FORMULA_SPEC_SCHEMA_V1, HOMEBREW_FORMULA_TEMPLATE_V1, HomebrewFormulaChannelSpec,
        HomebrewFormulaSpec, HomebrewHostProductSpec, HomebrewStableSentinel,
    };
    HomebrewFormulaSpec {
        schema: HOMEBREW_FORMULA_SPEC_SCHEMA_V1.to_owned(),
        template: HOMEBREW_FORMULA_TEMPLATE_V1.to_owned(),
        description: "Binary scanner".to_owned(),
        homepage: "https://github.com/example/scanner".to_owned(),
        license: "MIT".to_owned(),
        channels: vec![HomebrewFormulaChannelSpec {
            channel: velnor_actions_contract::package_release::PackageReleaseChannel::Preview,
            formula: "scanner-preview".to_owned(),
            formula_class: "ScannerPreview".to_owned(),
            destination: "Formula/scanner-preview.rb".to_owned(),
            conflicts: vec!["scanner".to_owned()],
        }],
        host: HomebrewHostProductSpec {
            product: "scanner".to_owned(),
            targets: vec!["x86_64-unknown-linux-gnu".to_owned()],
            bins: vec!["scanner".to_owned()],
        },
        auxiliaries: vec![],
        stable_sentinel: HomebrewStableSentinel {
            date: "2026-10-24".to_owned(),
            reason: "No stable release".to_owned(),
        },
    }
}
