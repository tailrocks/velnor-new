// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::{WorkflowsError, refuse};
use crate::expression::{Level, Scope};

const EVERYWHERE: &[&str] = &[
    "contains",
    "endsWith",
    "format",
    "fromJSON",
    "join",
    "startsWith",
    "toJSON",
];
const STATUS: &[&str] = &["always", "cancelled", "failure", "success"];
const HASH_FILES: &str = "hashFiles";

pub(crate) fn check_functions(
    inner: &str,
    names: &[String],
    scope: &Scope<'_>,
    condition: bool,
) -> Result<(), WorkflowsError> {
    for name in names {
        let listed = |list: &[&str]| list.iter().any(|known| known.eq_ignore_ascii_case(name));
        let available = listed(EVERYWHERE)
            || (condition && listed(STATUS))
            || (scope.level == Level::Step && HASH_FILES.eq_ignore_ascii_case(name));
        if !available {
            return refuse(format!(
                "the expression {inner} calls {name}(), which is not available here; use {EVERYWHERE:?}, in an if also {STATUS:?}, and in a step also {HASH_FILES}"
            ));
        }
    }
    Ok(())
}
