//! Explicit neutral bootstrap authority for orchestrator tests only.
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    ToolCacheDomain,
};
use velnor_actions_workflow_renderer::{MiseBootstrap, MiseSetup};

pub(crate) fn setup(version: &str, sha256: &str) -> MiseSetup {
    let domains = [
        ToolCacheDomain::Planning,
        ToolCacheDomain::Full,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ];
    let labels = [
        "ubuntu-26.04",
        "ubuntu-24.04",
        "ubuntu-22.04",
        "ubuntu-26.04-arm",
        "ubuntu-24.04-arm",
        "macos-26",
        "macos-15",
    ];
    let source = velnor_actions_contract::generated_source("0.1.0", "exit 0\n")
        .expect("neutral fixture source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let operation = SourceBoundOperation::MiseBootstrap;
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &digest)
        .expect("fixture descriptor");
    let invocation =
        HelperInvocation::compiled(descriptor, Vec::new(), Vec::new()).expect("fixture invocation");
    let mut bootstraps = BTreeMap::new();
    for domain in domains {
        for label in labels {
            let target = velnor_actions_contract::tool_target_for_runner_label(label)
                .expect("supported fixture runner target");
            let environment = BTreeMap::from([
                ("MISE_DATA_DIR".to_owned(), domain.root().to_owned()),
                ("VELNOR_MISE_SHA256".to_owned(), sha256.to_owned()),
                ("VELNOR_MISE_VERSION".to_owned(), version.to_owned()),
                ("VELNOR_MISE_TARGET".to_owned(), target.to_owned()),
                (
                    "VELNOR_QUALIFIED_TOOL_IDENTITY".to_owned(),
                    format!("qualified-tools@{}", "a".repeat(64)),
                ),
            ]);
            let helper = CompiledSourceHelper::compiled(invocation.clone(), source.clone())
                .expect("fixture record")
                .with_environment(environment);
            bootstraps.insert(
                (domain, label.to_owned()),
                MiseBootstrap {
                    target: target.to_owned(),
                    binary_sha256: sha256.to_owned(),
                    helper,
                },
            );
        }
    }
    MiseSetup {
        version: version.to_owned(),
        bootstraps,
    }
}
