// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

use crate::base_save::pinned;
use crate::facts::TestResult;
use crate::pipeline::workflow;

const GIT_LOCK: &str = "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/example/shared-lib?branch=main#0123456789abcdef\"\n";

fn incomplete_expressions(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("${{") {
        let after = &rest[start + 3..];
        let Some(end) = after.find("}}") else {
            found.push(after.lines().next().unwrap_or_default().to_owned());
            break;
        };
        let inner = after[..end].trim();
        let dangling = ["&&", "||", "!", "==", "!=", "(", ","]
            .iter()
            .any(|operator| inner.ends_with(operator));
        if inner.is_empty() || dangling {
            found.push(format!("${{{{{}}}}}", &after[..end]));
        }
        rest = &after[end + 2..];
    }
    found
}

#[test]
fn every_rendered_expression_is_complete_when_no_lane_uploads_a_cache() -> TestResult {
    let mut view = pinned("0.1.81", "")?;
    view.files
        .insert(RepoPath::new("Cargo.lock")?, GIT_LOCK.as_bytes().to_vec());
    let text = workflow(&view)?;
    assert!(text.contains("velnor-ci cache budget"), "{text}");
    assert_eq!(incomplete_expressions(&text), Vec::<String>::new());
    assert!(!text.contains("Require verified cache uploads"), "{text}");
    Ok(())
}
