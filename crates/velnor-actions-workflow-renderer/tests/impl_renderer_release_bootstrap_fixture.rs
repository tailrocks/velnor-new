//! Exact release bootstrap authority shared by renderer integration fixtures.
use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use velnor_actions_contract::{
    CompiledNativeExecRecipe, CompiledSourceHelper, CompiledSupportSource, HelperInvocation,
    SourceBoundHelper, SourceBoundOperation, ToolCacheDomain,
};
use velnor_actions_workflow_renderer::release_bootstrap::ReleaseBootstrapApproval;
use velnor_actions_workflow_renderer::{MiseBootstrap, MiseSetup};

pub(crate) const LABEL: &str = "ubuntu-26.04";
pub(crate) const MISE_VERSION: &str = "2026.9.18";
pub(crate) const MISE_SHA256: &str =
    "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4";

/// Build the one release-qualified Mise record used by every job fixture.
fn mise_helper(domain: ToolCacheDomain) -> CompiledSourceHelper {
    let source = velnor_actions_contract::generated_source("0.1.0", "exit 0\n")
        .expect("neutral fixture source");
    let source_sha256 = format!("{:x}", Sha256::digest(source.as_bytes()));
    let operation = SourceBoundOperation::MiseBootstrap;
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &source_sha256)
        .expect("fixture descriptor");
    let invocation =
        HelperInvocation::compiled(descriptor, Vec::new(), Vec::new()).expect("fixture invocation");
    let environment = BTreeMap::from([
        ("MISE_DATA_DIR".to_owned(), domain.root().to_owned()),
        ("VELNOR_MISE_SHA256".to_owned(), MISE_SHA256.to_owned()),
        ("VELNOR_MISE_VERSION".to_owned(), MISE_VERSION.to_owned()),
        (
            "VELNOR_MISE_TARGET".to_owned(),
            velnor_actions_contract::tool_target_for_runner_label(LABEL)
                .expect("fixture runner target")
                .to_owned(),
        ),
        (
            "VELNOR_QUALIFIED_TOOL_IDENTITY".to_owned(),
            format!("qualified-tools@{}", "a".repeat(64)),
        ),
    ]);
    CompiledSourceHelper::compiled(invocation, source)
        .expect("fixture record")
        .with_environment(environment)
}

fn mise_bootstrap(domain: ToolCacheDomain) -> MiseBootstrap {
    let helper = mise_helper(domain);
    MiseBootstrap {
        target: velnor_actions_contract::tool_target_for_runner_label(LABEL)
            .expect("fixture runner target")
            .to_owned(),
        binary_sha256: helper
            .environment()
            .get("VELNOR_MISE_SHA256")
            .expect("fixture Mise digest")
            .to_owned(),
        helper,
    }
}

pub(crate) fn mise() -> MiseSetup {
    MiseSetup {
        version: MISE_VERSION.to_owned(),
        bootstraps: [ToolCacheDomain::Full, ToolCacheDomain::Planning]
            .into_iter()
            .map(|domain| ((domain, LABEL.to_owned()), mise_bootstrap(domain)))
            .collect(),
    }
}

fn source_prepare_helper(domain: ToolCacheDomain, selector: &str) -> CompiledSourceHelper {
    let operation = SourceBoundOperation::MiseToolPrepare;
    let source =
        velnor_actions_contract::generated_source("0.1.0", &format!("exit 0\n# {operation:?}\n"))
            .expect("source preparation source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &digest)
        .expect("source preparation descriptor");
    let selectors = vec![selector.to_owned()];
    let invocation = HelperInvocation::compiled(descriptor, Vec::new(), selectors.clone())
        .expect("source preparation invocation");
    let environment = BTreeMap::from([("MISE_DATA_DIR".to_owned(), domain.root().to_owned())]);
    let mut prefix = vec![
        "env".to_owned(),
        "-i".to_owned(),
        "/owned/mise".to_owned(),
        "exec".to_owned(),
    ];
    prefix.extend(selectors.iter().cloned());
    prefix.push("--".to_owned());
    let recipe = CompiledNativeExecRecipe::compiled_for_scope(
        prefix,
        environment.clone(),
        selectors,
        velnor_actions_contract::workflow::native_tools::NativeCredentialScope::Anonymous,
    )
    .expect("source preparation recipe");
    CompiledSourceHelper::compiled(invocation, source)
        .expect("source preparation helper")
        .with_environment(environment)
        .with_execution_recipe(recipe)
        .expect("source preparation execution recipe")
}

pub(crate) fn source_tool_records() -> Vec<CompiledSourceHelper> {
    let setup = mise();
    vec![
        setup
            .bootstrap(ToolCacheDomain::Full, LABEL)
            .expect("full source bootstrap")
            .helper
            .clone(),
        setup
            .bootstrap(ToolCacheDomain::Planning, LABEL)
            .expect("planning source bootstrap")
            .helper
            .clone(),
        source_prepare_helper(ToolCacheDomain::Full, "python@3.14.7"),
        source_prepare_helper(ToolCacheDomain::Planning, "gh@2.102.0"),
    ]
}

pub(crate) fn prepared_tool_records() -> Vec<CompiledSourceHelper> {
    let records = source_tool_records();
    vec![records[0].clone(), records[2].clone()]
}

/// Build the complete frozen release approval consumed by every fixture spec.
pub(crate) fn bootstrap_tools(checkout_uses: String) -> ReleaseBootstrapApproval {
    ReleaseBootstrapApproval {
        checkout_uses,
        mise: mise(),
    }
}

/// Supply only the exact bootstrap record required by every release job.
pub(crate) fn helper_registry() -> Vec<CompiledSourceHelper> {
    vec![
        mise()
            .bootstrap(ToolCacheDomain::Full, LABEL)
            .expect("fixture bootstrap")
            .helper
            .clone(),
    ]
}

/// Supply the complete neutral source inventory for tree rendering fixtures.
pub(crate) fn support_sources() -> Vec<CompiledSupportSource> {
    velnor_actions_workflow_renderer::release_tree::RELEASE_TREE_PATHS
        .iter()
        .filter(|path| path.starts_with(".github/velnor/"))
        .map(|path| {
            CompiledSupportSource::compiled(path, "fixture source\n", "0.1.0")
                .expect("support source")
        })
        .collect()
}
