use std::collections::BTreeMap;

use velnor_actions_contract::{
    BUN_REGISTRY_INSTALL_OPERATION, PRIVATE_NPM_READ_EXPR, PRIVATE_NPM_READ_TOKEN_ENV, Step,
    StepKind, StepRole,
};

use crate::RenderError;

pub const BUN_REGISTRY_STEP_NAME: &str = "Install dependencies (registry phase)";
pub const BUN_ROOT_ENV: &str = "VELNOR_BUN_ROOT";
pub const BUN_SCOPES_ENV: &str = "VELNOR_BUN_SCOPES";

fn bad(detail: &str) -> RenderError {
    RenderError::BadCommand(format!("bun_registry:{detail}"))
}

pub fn bun_registry_step(
    root: &str,
    scopes: &[String],
    toolchain_env: &BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if scopes.is_empty() || scopes.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(bad("scopes"));
    }
    let scope_ok = |scope: &String| {
        scope.strip_prefix('@').is_some_and(|name| {
            !name.is_empty()
                && name.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
                })
        })
    };
    if !scopes.iter().all(scope_ok) {
        return Err(bad("scope"));
    }
    let root = if root.is_empty() { "." } else { root };
    if root != "."
        && !root.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
    {
        return Err(bad("root"));
    }
    if toolchain_env
        .keys()
        .any(|key| crate::toolchain_env::is_denied_credential_key(key))
    {
        return Err(bad("toolchain_env_credential"));
    }
    let mut env = toolchain_env.clone();
    env.extend([
        (
            PRIVATE_NPM_READ_TOKEN_ENV.to_owned(),
            PRIVATE_NPM_READ_EXPR.to_owned(),
        ),
        (BUN_ROOT_ENV.to_owned(), root.to_owned()),
        (BUN_SCOPES_ENV.to_owned(), scopes.join(",")),
    ]);
    Ok(Step {
        parallel: false,
        name: BUN_REGISTRY_STEP_NAME.to_owned(),
        id: None,
        role: Some(StepRole::BunRegistryInstall),
        condition: None,
        kind: StepKind::Internal {
            operation: BUN_REGISTRY_INSTALL_OPERATION.to_owned(),
            env,
        },
    })
}
