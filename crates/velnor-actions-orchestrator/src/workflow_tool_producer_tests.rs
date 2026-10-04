use super::*;

fn descriptor(domain: ToolCacheDomain) -> ToolCacheDescriptor {
    ToolCacheDescriptor {
        domain,
        target: "x86_64-unknown-linux-gnu".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        selectors: vec!["gh@2.86.0".to_owned()],
        immutable_identity: "mise-v3-test".to_owned(),
        qualification_identity: format!("qualified-tools@{}", "1".repeat(64)),
    }
}

#[test]
fn pure_domain_identity_separates_equal_selectors_at_distinct_payload_roots() {
    let full = descriptor(ToolCacheDomain::Full);
    let planning = descriptor(ToolCacheDomain::Planning);
    assert_ne!(
        producer_id(&full).expect("full"),
        producer_id(&planning).expect("planning")
    );
    assert_ne!(full.domain.payload(), planning.domain.payload());
}

#[test]
fn metadata_restore_binding_matches_fixed_observer_domain() {
    for domain in [
        ToolCacheDomain::Planning,
        ToolCacheDomain::Full,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ] {
        let meta =
            metadata(&descriptor(domain), ToolProducerSelection::default()).expect("metadata");
        assert_eq!(
            meta.restore_step.as_str(),
            velnor_actions_contract::CacheSnapshotDomain::tool_domain(domain).restore_id()
        );
        assert!(meta.validate().is_ok());
    }
}

#[test]
fn compilation_and_repair_schedule_reads_actual_plan_result() {
    let selection = ToolProducerSelection {
        tasks: vec!["stack/rust/example/test/default".to_owned()],
        cargo_fallback: true,
        unconditional: false,
    };
    let condition = selection.condition(ToolCacheDomain::Full);
    assert!(condition.contains("needs.plan.result == 'success'"));
    assert!(condition.contains("needs.plan.outputs.cargo_fallback_required == 'true'"));
    assert!(condition.contains("!contains(needs.plan.outputs.covered_tasks"));
    assert_eq!(selection.needs(ToolCacheDomain::Full), ["plan"]);
    assert!(selection.needs(ToolCacheDomain::Planning).is_empty());
}
