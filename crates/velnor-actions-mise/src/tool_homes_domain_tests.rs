//! Home authority is fixed by cache domain, independent of selected tools.

use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::ToolCacheDomain;

use super::ToolHomes;
use crate::{MiseError, ToolCatalog};

fn pairs<const N: usize>(entries: [(&str, &str); N]) -> Vec<(OsString, OsString)> {
    entries
        .into_iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect()
}

fn assert_exact_environment(
    actual: Vec<(OsString, OsString)>,
    expected: Vec<(OsString, OsString)>,
) {
    assert_eq!(actual.len(), expected.len(), "missing or duplicate keys");
    let actual: BTreeMap<_, _> = actual.into_iter().collect();
    let expected: BTreeMap<_, _> = expected.into_iter().collect();
    assert_eq!(actual, expected, "changed home authority");
}

#[test]
fn runner_temp_home_environment_has_all_four_canonical_bindings() {
    assert_exact_environment(
        ToolHomes::runner_temp().home_env(),
        pairs([
            ("MISE_RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup"),
            ("MISE_CARGO_HOME", "${{ runner.temp }}/velnor/cargo"),
            ("RUSTUP_HOME", "${{ runner.temp }}/velnor/rustup"),
            ("CARGO_HOME", "${{ runner.temp }}/velnor/cargo"),
        ]),
    );
}

#[test]
fn custom_home_values_are_preserved_for_mise_and_direct_launches() -> Result<(), MiseError> {
    let homes = ToolHomes::new("/tmp/owned rustup", "/tmp/cargo 'owned'")?;
    assert_exact_environment(
        homes.home_env(),
        pairs([
            ("MISE_RUSTUP_HOME", "/tmp/owned rustup"),
            ("MISE_CARGO_HOME", "/tmp/cargo 'owned'"),
            ("RUSTUP_HOME", "/tmp/owned rustup"),
            ("CARGO_HOME", "/tmp/cargo 'owned'"),
        ]),
    );
    Ok(())
}

#[test]
fn full_domain_homes_match_canonical_homes_without_tool_selection() {
    assert_exact_environment(
        ToolHomes::domain_home_env(ToolCacheDomain::Full),
        ToolHomes::runner_temp().home_env(),
    );
}

#[test]
fn every_other_domain_has_no_rust_or_cargo_home_authority() {
    for domain in [
        ToolCacheDomain::Planning,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ] {
        assert!(
            ToolHomes::domain_home_env(domain).is_empty(),
            "foreign Rust/Cargo homes in {}",
            domain.name(),
        );
    }
}

#[test]
fn toolchain_environment_preserves_home_authority_and_catalog_pin() {
    let homes = ToolHomes::runner_temp();
    let catalog = ToolCatalog::pinned();
    let mut expected = homes.home_env();
    expected.extend(pairs([
        ("MISE_DATA_DIR", ToolCacheDomain::Full.root()),
        ("RUSTUP_TOOLCHAIN", &catalog.rustup_toolchain()),
    ]));
    assert_exact_environment(homes.env(&catalog), expected);
}

#[test]
fn prepare_and_exec_environments_preserve_every_home_binding() -> Result<(), MiseError> {
    let homes = ToolHomes::new("/tmp/custom/rustup", "/tmp/custom/cargo")?;
    let catalog = ToolCatalog::pinned();
    for environment in [homes.prepare_env(&catalog), homes.exec_env(&catalog)] {
        for (key, value) in homes.home_env() {
            let bindings: Vec<_> = environment
                .iter()
                .filter(|(candidate, _)| candidate == &key)
                .collect();
            assert_eq!(bindings.len(), 1, "missing or duplicate home {key:?}");
            assert_eq!(&bindings[0].1, &value, "changed home {key:?}");
        }
    }
    Ok(())
}
