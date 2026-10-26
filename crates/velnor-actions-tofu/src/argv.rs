// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;

use velnor_actions_contract::ContractError;

use crate::kinds::TofuTaskKind;

pub const CHDIR_FINDING_TAG: &str = "path.cwd";

pub fn tofu_payload_argv(kind: TofuTaskKind, root: &str) -> Result<Vec<OsString>, ContractError> {
    let mut argv: Vec<OsString> = Vec::new();
    if !root.is_empty() {
        reject_leading_dash(root, "root")?;
        if root.split('/').any(|segment| segment == "..") {
            return Err(ContractError::identity("tofu_cli_argv", "traversal_root"));
        }
        argv.push(OsString::from("-chdir"));
        argv.push(OsString::from(root));
    }
    let flag = OsString::from;
    match kind {
        TofuTaskKind::Fmt => {
            argv.extend([flag("fmt"), flag("-check"), flag("-recursive")]);
            argv.push(flag("-no-color"));
        }
        TofuTaskKind::InitForValidate => {
            argv.extend([flag("init"), flag("-backend=false")]);
            argv.extend([flag("-input=false"), flag("-lockfile=readonly")]);
            argv.push(flag("-no-color"));
        }
        TofuTaskKind::Validate => {
            argv.extend([flag("validate"), flag("-no-color")]);
        }
    }
    Ok(argv)
}

#[must_use]
pub fn chdir_finding_for_root(root: &str) -> Option<String> {
    if root.is_empty() {
        None
    } else {
        Some(format!("{CHDIR_FINDING_TAG}:{root}"))
    }
}

fn reject_leading_dash(value: &str, what: &str) -> Result<(), ContractError> {
    if value.starts_with('-') {
        return Err(ContractError::identity(
            "tofu_argv",
            format!("leading_dash_{what}"),
        ));
    }
    Ok(())
}
