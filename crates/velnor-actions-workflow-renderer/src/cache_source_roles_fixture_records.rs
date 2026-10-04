use super::{SOURCE_IDENTITY, TARGET};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
    ToolCacheDomain,
};

fn owner_environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "MISE_DATA_DIR".to_owned(),
            ToolCacheDomain::NpmBootstrap.root().to_owned(),
        ),
        (
            "VELNOR_SOURCE_IDENTITY".to_owned(),
            SOURCE_IDENTITY.to_owned(),
        ),
        (
            "VELNOR_QUALIFIED_TOOL_IDENTITY".to_owned(),
            format!("qualified-tools@{}", "a".repeat(64)),
        ),
        ("VELNOR_MISE_SHA256".to_owned(), "a".repeat(64)),
    ])
}

fn tool_environment(selectors: &[&str]) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("RUSTUP_AUTO_INSTALL".to_owned(), "0".to_owned()),
        (
            "MISE_DATA_DIR".to_owned(),
            ToolCacheDomain::NpmBootstrap.root().to_owned(),
        ),
        ("VELNOR_MISE_SHA256".to_owned(), "a".repeat(64)),
        (
            "VELNOR_TOOL_CACHE_IDENTITY".to_owned(),
            format!(
                "toolset@{}",
                velnor_actions_contract::digest_b3(selectors.join("\0").as_bytes())
            ),
        ),
        (
            "VELNOR_QUALIFIED_TOOL_IDENTITY".to_owned(),
            format!("qualified-tools@{}", "a".repeat(64)),
        ),
    ])
}

pub(super) fn compiled_record(
    operation: SourceBoundOperation,
    selectors: Vec<String>,
    environment: BTreeMap<String, String>,
) -> CompiledSourceHelper {
    let source =
        velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("generated source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor = SourceBoundHelper::compiled(operation, operation.path(), &digest)
        .expect("compiled descriptor");
    let args: Vec<String> = if operation == SourceBoundOperation::MiseToolPrepare {
        std::iter::once("npm-bootstrap".to_owned())
            .chain(std::iter::once(TARGET.to_owned()))
            .chain(selectors.iter().cloned())
            .collect()
    } else {
        environment
            .get("VELNOR_SNAPSHOT_PHASE")
            .cloned()
            .map_or_else(
                || vec!["owned-source-input".to_owned()],
                |phase| vec![environment["VELNOR_SNAPSHOT_LAYER"].clone(), phase],
            )
    };
    let invocation =
        HelperInvocation::compiled(descriptor, args, selectors).expect("compiled invocation");
    CompiledSourceHelper::compiled(invocation, source)
        .expect("compiled source owner")
        .with_environment(environment)
}

pub(super) fn owner_record(
    operation: SourceBoundOperation,
    selectors: &[&str],
) -> CompiledSourceHelper {
    let environment = if operation == SourceBoundOperation::MiseToolPrepare {
        tool_environment(selectors)
    } else {
        owner_environment()
    };
    compiled_record(
        operation,
        selectors.iter().map(|value| (*value).to_owned()).collect(),
        environment,
    )
}
