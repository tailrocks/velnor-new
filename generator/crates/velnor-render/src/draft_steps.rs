// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::CI_RUN_LABEL;

use crate::rules::{RenderedStep, Value};

const IS_DRAFT: &str = "${{ github.event.pull_request.draft == true }}";
const DRAFT_NOTICE: &str = "echo \"::notice title=Draft::draft: CI not requested\"";

pub(crate) fn draft_notice() -> RenderedStep {
    RenderedStep {
        name: "Note that CI is not requested on a draft pull request".to_owned(),
        condition: Some(IS_DRAFT.to_owned()),
        run: Some(DRAFT_NOTICE.to_owned()),
        ..RenderedStep::default()
    }
}

pub(crate) fn clear_run_label() -> RenderedStep {
    RenderedStep {
        name: format!("Remove the {CI_RUN_LABEL} label"),
        env: vec![
            (
                "GH_TOKEN".to_owned(),
                Value::Expr("${{ github.token }}".to_owned()),
            ),
            (
                "PR_NUMBER".to_owned(),
                Value::Expr("${{ github.event.pull_request.number }}".to_owned()),
            ),
        ],
        run: Some(format!(
            "curl --fail --silent --show-error --request DELETE --header \"Authorization: Bearer $GH_TOKEN\" --header \"Accept: application/vnd.github+json\" \"$GITHUB_API_URL/repos/$GITHUB_REPOSITORY/issues/$PR_NUMBER/labels/{}\"",
            CI_RUN_LABEL.replace(':', "%3A")
        )),
        ..RenderedStep::default()
    }
}

pub(crate) fn fail_on_draft() -> RenderedStep {
    RenderedStep {
        name: "Fail on a draft pull request, whose jobs were skipped".to_owned(),
        condition: Some(IS_DRAFT.to_owned()),
        run: Some("exit 1".to_owned()),
        ..RenderedStep::default()
    }
}
