use super::*;

fn descriptor() -> ToolCacheDescriptor {
    ToolCacheDescriptor {
        domain: ToolCacheDomain::Planning,
        target: "x86_64-unknown-linux-gnu".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        selectors: vec!["gh@2.87.3".to_owned()],
        immutable_identity: "mise-v3-planning-x86_64-unknown-linux-gnu-qualified".to_owned(),
        qualification_identity: format!("qualified-tools@{}", "a".repeat(64)),
    }
}

#[test]
fn canonical_domains_never_archive_repository_or_source_state() {
    for domain in [
        ToolCacheDomain::Planning,
        ToolCacheDomain::Full,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ] {
        let paths = domain.payload();
        assert!(
            paths
                .iter()
                .all(|path| path.starts_with("${{ runner.temp }}/velnor/"))
        );
        assert!(paths.iter().all(|path| !path.contains("registry")
            && !path.contains("native/")
            && !path.contains("target")
            && !path.contains("..")));
        assert!(paths.contains(&domain.root().to_owned()));
    }
}

#[test]
fn mutable_qualification_runner_and_transport_are_rejected() {
    assert!(descriptor().validate().is_ok());
    let mut wrong_target = descriptor();
    wrong_target.target = "aarch64-apple-darwin".to_owned();
    assert!(wrong_target.validate().is_err());
    for invalid in [
        "",
        "qualified-tools@floating",
        "qualified-tools@${{ env.DIGEST }}",
        "qualified-tools@aa\nbb",
    ] {
        let mut meta = descriptor();
        meta.qualification_identity = invalid.to_owned();
        assert!(meta.validate().is_err(), "{invalid}");
    }
    for invalid in ["ubuntu-latest", "${{ matrix.os }}", ""] {
        let mut meta = descriptor();
        meta.runs_on = invalid.to_owned();
        assert!(meta.validate().is_err(), "{invalid}");
    }
    for invalid in ["", "shared-${{ env.KEY }}", "key\nother"] {
        let mut meta = descriptor();
        meta.immutable_identity = invalid.to_owned();
        assert!(meta.validate().is_err(), "{invalid}");
    }
}

#[test]
fn descriptor_footprint_is_sorted_unique_bounded_and_literal() {
    for invalid in [
        Vec::new(),
        vec!["node@24.0.0".to_owned(), "gh@2.87.3".to_owned()],
        vec!["gh@2.87.3".to_owned(), "gh@2.87.3".to_owned()],
        vec!["gh@${{ env.VERSION }}".to_owned()],
        vec!["gh@2.87.3\ninjected".to_owned()],
    ] {
        let mut meta = descriptor();
        meta.selectors = invalid;
        assert!(meta.validate().is_err());
    }
    let mut meta = descriptor();
    meta.selectors = vec!["a".repeat(524_289)];
    assert!(meta.validate().is_err());
}

#[test]
fn wire_metadata_cannot_add_an_arbitrary_root_or_operation() {
    let mut value = serde_json::to_value(descriptor()).expect("descriptor");
    value["root"] = serde_json::json!("${{ github.workspace }}");
    assert!(serde_json::from_value::<ToolCacheDescriptor>(value).is_err());
    assert!(
        serde_json::from_value::<ToolCacheDomain>(serde_json::json!("repository-tools")).is_err()
    );
}

#[test]
fn evidence_cannot_alias_the_fixed_publication_receipt() {
    let id = |value: &str| StepId::new(value).expect("id");
    let mut meta = PureToolProducer {
        descriptor: descriptor(),
        selection: super::super::tool_producer_selection::ToolProducerSelection::default(),
        restore_step: id("restore"),
        before_step: id("before"),
        installation_step: id("install"),
        after_step: id("after"),
        save_step: id("save"),
        report_step: id("report"),
    };
    assert!(meta.validate().is_ok());
    meta.report_step = id("velnor-tool-publication");
    assert!(meta.validate().is_err());
}
