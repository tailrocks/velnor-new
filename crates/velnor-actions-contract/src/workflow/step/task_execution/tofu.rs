// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{TaskExecutionValidation, argv::Invocation};
use crate::{ContractError, canonical_json_bytes, digest_b3};

pub(super) fn validate(
    input: &TaskExecutionValidation<'_>,
    invocation: &Invocation,
) -> Result<(), ContractError> {
    let parts: Vec<_> = input.task_id.split('/').collect();
    let ["stack", "tofu", key, kind, "default"] = parts.as_slice() else {
        return Err(bad("invalid_tofu_task"));
    };
    let root = root_for_key(key)?;
    let selector = &input.argv[5];
    if invocation.separator != 6
        || !invocation.selectors.contains("opentofu")
        || input.toolchain_inputs.tools != [selector.clone()]
        || input.toolchain_inputs.compile_driver != "tofu"
        || input.toolchain_inputs.test_runner != "none"
    {
        return Err(bad("invalid_tofu_toolchain"));
    }
    let suffix: &[&str] = match *kind {
        "fmt" => &["fmt", "-check", "-recursive", "-no-color"],
        "init" => &[
            "init",
            "-backend=false",
            "-input=false",
            "-lockfile=readonly",
            "-no-color",
        ],
        "validate" => &["validate", "-no-color"],
        _ => return Err(bad("invalid_tofu_kind")),
    };
    let mut payload = vec!["tofu".to_owned()];
    if !root.is_empty() {
        payload.extend(["-chdir".to_owned(), root]);
    }
    payload.extend(suffix.iter().map(|arg| (*arg).to_owned()));
    if input.argv[7..] != payload {
        return Err(bad("invalid_tofu_payload"));
    }
    let component = serde_json::json!({
        "schema": "tofu-provider-inputs-v1",
        "unit": key,
        "kind_slot": if *kind == "fmt" { "excluded:kind_does_not_read_lockfile" } else { "lockfile" },
        "tofu": selector,
    });
    let component = format!(
        "tofu-provider-inputs:{}",
        digest_b3(&canonical_json_bytes(&component)?)
    );
    if input.toolchain_inputs.components != [component] {
        return Err(bad("invalid_tofu_provider_inputs"));
    }
    validate_env(input, key)
}

fn root_for_key(key: &str) -> Result<String, ContractError> {
    let hex = key
        .strip_prefix("dir-")
        .ok_or_else(|| bad("invalid_tofu_root_key"))?;
    if hex.len() % 2 != 0 || !crate::ids::is_lower_hex(hex) {
        return Err(bad("invalid_tofu_root_key"));
    }
    let bytes = hex
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| {
                if byte <= b'9' {
                    byte - b'0'
                } else {
                    byte - b'a' + 10
                }
            };
            (digit(pair[0]) << 4) | digit(pair[1])
        })
        .collect();
    let root = String::from_utf8(bytes).map_err(|_| bad("invalid_tofu_root_key"))?;
    if !root.is_empty() {
        crate::config::Utf8RepoRelDir::parse(&root).map_err(|_| bad("invalid_tofu_root"))?;
        if root == "." || root.starts_with('-') {
            return Err(bad("invalid_tofu_root"));
        }
    }
    Ok(root)
}

fn validate_env(input: &TaskExecutionValidation<'_>, key: &str) -> Result<(), ContractError> {
    let locator = digest_b3(format!("tofu-private-root-v1\n{key}").as_bytes());
    let expected = std::collections::BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("TF_INPUT".to_owned(), "0".to_owned()),
        ("TF_IN_AUTOMATION".to_owned(), "1".to_owned()),
        (
            "TF_DATA_DIR".to_owned(),
            format!("${{{{ runner.temp }}}}/velnor/tofu-data/{locator}"),
        ),
        (
            "TF_PLUGIN_CACHE_DIR".to_owned(),
            format!("${{{{ runner.temp }}}}/velnor/tofu-cache/{locator}"),
        ),
    ]);
    if input.env != &expected {
        return Err(bad("invalid_tofu_environment"));
    }
    Ok(())
}

fn bad(reason: &str) -> ContractError {
    ContractError::identity("step.task.tofu", reason)
}
