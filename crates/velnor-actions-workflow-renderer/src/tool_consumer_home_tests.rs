use super::native_install_environment;
use super::tests::base;
use velnor_actions_contract::ToolCacheDomain;

#[test]
fn full_requires_every_exact_canonical_home() {
    let domain = ToolCacheDomain::Full;
    let mut environment = base();
    assert!(!native_install_environment(&environment, domain));
    let homes = domain.home_environment();
    assert_eq!(homes.len(), 4);
    environment.extend(homes.clone());
    assert!(native_install_environment(&environment, domain));
    for (key, value) in &homes {
        let mut missing = environment.clone();
        missing.remove(key);
        assert!(
            !native_install_environment(&missing, domain),
            "missing {key}"
        );
        for foreign in [
            "",
            "/tmp/unowned",
            "$HOME",
            value.trim_end_matches("rustup"),
        ] {
            if foreign == value.as_str() {
                continue;
            }
            let mut altered = environment.clone();
            altered.insert(key.clone(), foreign.to_owned());
            assert!(
                !native_install_environment(&altered, domain),
                "altered {key}"
            );
        }
    }
}

#[test]
fn non_full_domains_reject_full_homes() {
    for domain in [
        ToolCacheDomain::Planning,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ] {
        let mut environment = base();
        if domain == ToolCacheDomain::GradleBootstrap {
            environment.extend(
                super::GRADLE
                    .into_iter()
                    .map(|key| (key.to_owned(), "owner-bound".to_owned())),
            );
        }
        assert!(native_install_environment(&environment, domain));
        for (key, value) in ToolCacheDomain::Full.home_environment() {
            let mut altered = environment.clone();
            altered.insert(key.clone(), value);
            assert!(
                !native_install_environment(&altered, domain),
                "foreign {key}"
            );
        }
    }
}

#[test]
fn full_same_count_unknown_key_cannot_replace_home() {
    let mut environment = base();
    environment.extend(ToolCacheDomain::Full.home_environment());
    let home = ToolCacheDomain::Full
        .home_environment()
        .into_keys()
        .next()
        .expect("home");
    environment.remove(&home);
    environment.insert("UNOWNED_HOME".to_owned(), "same-count".to_owned());
    assert!(!native_install_environment(
        &environment,
        ToolCacheDomain::Full
    ));
}
