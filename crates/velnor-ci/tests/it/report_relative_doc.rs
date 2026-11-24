// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use velnor_ci::report::waste;

use super::report::{artifact, metrics, workspace_app};

#[test]
fn relative_doc_page_with_absolute_target_is_not_waste() {
    let value = workspace_app(
        json!([artifact(
            "path+file:///checkout/app#1.0.0",
            false,
            &["target/doc/app/index.html"]
        )]),
        Some("/home/runner/_work/repo/repo/target"),
    );
    assert_eq!(waste(&metrics(value)), [] as [&str; 0]);
}
