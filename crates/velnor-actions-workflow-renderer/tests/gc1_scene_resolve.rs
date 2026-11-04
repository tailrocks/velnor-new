// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::Path;

use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_KEY_EXPR, MBX_BUNDLE_MATCHED_KEY_EXPR,
};

use crate::gc1_scene::fail;

pub(crate) fn resolve(
    text: &str,
    matched: &str,
    selected: &str,
    runner: &Path,
) -> Result<String, Box<dyn Error>> {
    let strip = |expr: &str| -> String {
        expr.strip_prefix("${{")
            .and_then(|rest| rest.strip_suffix("}}"))
            .unwrap_or(expr)
            .trim()
            .to_owned()
    };
    let matched_expr = strip(MBX_BUNDLE_MATCHED_KEY_EXPR);
    let selected_expr = strip(MBX_BUNDLE_KEY_EXPR);
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("${{") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 3..];
        let end = after
            .find("}}")
            .ok_or_else(|| fail("unterminated expression"))?;
        let inner = after[..end].trim();
        if inner == matched_expr {
            out.push_str(matched);
        } else if inner == selected_expr {
            out.push_str(selected);
        } else if inner == "runner.temp" {
            out.push_str(&runner.display().to_string());
        } else {
            return Err(fail(&format!("unresolved expression {inner}")));
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}
