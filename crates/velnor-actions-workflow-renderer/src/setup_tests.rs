//! Neutral acquisition bindings reject host, domain and pin substitutions.
use super::{fixture::mise_setup, *};

#[test]
fn acquisition_uses_actual_host_domain_and_no_managed_selectors() {
    let setup = mise_setup("2026.10.0", &"a".repeat(64));
    let domain = ToolCacheDomain::Planning;
    let pin = setup.bootstrap(domain, "ubuntu-26.04").expect("pin");
    let step = mise_setup_step(&setup, domain, "ubuntu-26.04").expect("step");
    assert_eq!(step.name, SETUP_MISE_NAME);
    let velnor_actions_contract::StepKind::SourceBoundHelper { invocation, env } = step.kind else {
        panic!("acquisition must be source bound");
    };
    assert_eq!(&invocation, pin.helper.invocation());
    assert_eq!(&env, pin.helper.environment());
    assert!(invocation.installed_selectors().is_empty());
    assert!(setup.bootstrap(domain, "unqualified-host").is_err());
}

#[test]
fn absence_and_detached_binary_pin_fail_closed() {
    let mut setup = mise_setup("2026.10.0", &"a".repeat(64));
    setup.bootstraps.clear();
    assert!(setup.validate().is_err());
    let mut setup = mise_setup("2026.10.0", &"a".repeat(64));
    setup
        .bootstraps
        .values_mut()
        .next()
        .expect("fixture")
        .binary_sha256 = "b".repeat(64);
    assert!(setup.validate().is_err());
}

#[test]
fn changed_domain_version_and_bootstrap_selectors_fail_closed() {
    for key in ["MISE_DATA_DIR", "VELNOR_MISE_VERSION", "VELNOR_MISE_TARGET"] {
        let mut setup = mise_setup("2026.10.0", &"a".repeat(64));
        let pin = setup.bootstraps.values_mut().next().expect("fixture");
        let mut env = pin.helper.environment().clone();
        env.insert(key.to_owned(), "changed".to_owned());
        pin.helper = pin.helper.clone().with_environment(env);
        assert!(setup.validate().is_err(), "accepted changed {key}");
    }
    let mut setup = mise_setup("2026.10.0", &"a".repeat(64));
    let pin = setup.bootstraps.values_mut().next().expect("fixture");
    let invocation = velnor_actions_contract::HelperInvocation::compiled(
        pin.helper.invocation().descriptor().clone(),
        Vec::new(),
        vec!["gh@2.102.0".to_owned()],
    )
    .expect("fixture invocation");
    pin.helper = CompiledSourceHelper::compiled(invocation, pin.helper.source().to_owned())
        .expect("fixture record")
        .with_environment(pin.helper.environment().clone());
    assert!(setup.validate().is_err());
}

#[test]
fn genuine_linux_authority_cannot_be_rekeyed_to_macos() {
    let mut setup = mise_setup("2026.10.0", &"a".repeat(64));
    let domain = ToolCacheDomain::Full;
    let linux = setup
        .bootstrap(domain, "ubuntu-26.04")
        .expect("linux")
        .clone();
    setup
        .bootstraps
        .insert((domain, "macos-26".to_owned()), linux);
    assert!(setup.validate().is_err());
}
