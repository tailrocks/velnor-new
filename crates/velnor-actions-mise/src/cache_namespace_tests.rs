//! Canonical namespace generation does not create runtime authority.

use super::*;

#[test]
fn every_domain_uses_exact_fixed_source_binding() {
    for domain in CacheSnapshotDomain::ALL {
        let descriptor = CacheNamespaceDescriptor::for_domain(domain);
        assert_eq!(descriptor.domain(), domain);
        assert_eq!(descriptor.root_expression(), "${{ runner.temp }}/velnor");
        assert_eq!(
            descriptor.namespace_environment(),
            ("VELNOR_CACHE_PAYLOAD_ROOT", "${{ runner.temp }}/velnor")
        );
        assert_eq!(descriptor.runner_temp_environment(), "RUNNER_TEMP");
        assert_eq!(descriptor.roots(), domain.roots());
        assert!(std::ptr::eq(descriptor.roots(), domain.roots()));
    }
}

#[test]
fn shared_cargo_parent_never_becomes_an_owned_payload_root() {
    let tools = CacheNamespaceDescriptor::for_domain(CacheSnapshotDomain::Tools);
    let sources = CacheNamespaceDescriptor::for_domain(CacheSnapshotDomain::Sources);
    assert!(tools.roots().iter().any(|root| root.starts_with("cargo/")));
    assert!(
        sources
            .roots()
            .iter()
            .any(|root| root.starts_with("cargo/"))
    );
    for tool in tools.roots() {
        for source in sources.roots() {
            assert_ne!(tool, source);
            assert!(!tool.starts_with(&format!("{source}/")));
            assert!(!source.starts_with(&format!("{tool}/")));
        }
    }
    assert!(!tools.roots().contains(&"cargo"));
    assert!(!sources.roots().contains(&"cargo"));
}

#[test]
fn closed_domains_have_distinct_namespace_descriptors() {
    for (index, left) in CacheSnapshotDomain::ALL.iter().enumerate() {
        for right in &CacheSnapshotDomain::ALL[index + 1..] {
            assert_ne!(
                CacheNamespaceDescriptor::for_domain(*left),
                CacheNamespaceDescriptor::for_domain(*right)
            );
        }
    }
}

#[test]
fn contract_roots_remain_relative_and_have_one_owner() {
    for (index, domain) in CacheSnapshotDomain::ALL.iter().enumerate() {
        for root in CacheNamespaceDescriptor::for_domain(*domain).roots() {
            assert!(!root.is_empty());
            assert!(!root.starts_with('/'));
            assert!(root.split('/').all(|part| !matches!(part, "" | "." | "..")));
            for other in &CacheSnapshotDomain::ALL[index + 1..] {
                for other_root in CacheNamespaceDescriptor::for_domain(*other).roots() {
                    assert_ne!(root, other_root);
                    assert!(!root.starts_with(&format!("{other_root}/")));
                    assert!(!other_root.starts_with(&format!("{root}/")));
                }
            }
        }
    }
}
