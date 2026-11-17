// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::{RenderedStep, Value};
use crate::steps::{DOWNLOAD_ARTIFACT, UPLOAD_ARTIFACT};
use velnor_model::CacheKey;

const REPORT_STEP: &str = "report_cache_metrics";

pub(crate) fn cache_id(key: &CacheKey, restore: bool) -> String {
    if let CacheKey::Sources { root, .. } = key
        && !restore
    {
        return crate::gate::sources_save_step(root.slug());
    }
    let root = match key {
        CacheKey::Tools { .. } => None,
        CacheKey::Sources { root, .. }
        | CacheKey::Target { root, .. }
        | CacheKey::Delta { root, .. } => Some(root),
    };
    let family = if key.family() == "tools" {
        "tools"
    } else if key.family() == "sources" {
        "sources"
    } else {
        key.family()
    };
    format!(
        "{}{}{}",
        if restore { "" } else { "save_" },
        family,
        root.map_or_else(String::new, |root| format!("_{}", root.slug()))
    )
}

pub(crate) fn lane(caches: &[CacheKey], tests: bool) -> RenderedStep {
    let caches: Vec<_> = caches.iter().map(|key| {
        let root = match key { CacheKey::Tools { .. } => None, CacheKey::Sources { root, .. } | CacheKey::Target { root, .. } | CacheKey::Delta { root, .. } => Some(root.lock_file().parent().map_or_else(|| ".".to_owned(), |root| root.to_string())) };
        serde_json::json!({"family":key.family(),"root":root,"restore_step":cache_id(key,true),"save_step":cache_id(key,false)})
    }).collect();
    RenderedStep {
        id: Some(REPORT_STEP.to_owned()),
        name: "Report cache telemetry".to_owned(),
        continue_on_error: true,
        run: Some("velnor-ci report".to_owned()),
        env: vec![
            (
                "VELNOR_REPORT_CACHES".to_owned(),
                Value::Text(
                    serde_json::Value::Array(caches)
                        .to_string()
                        .replace('$', "\\u0024"),
                ),
            ),
            (
                "VELNOR_REPORT_STEPS".to_owned(),
                Value::Expr("${{ toJSON(steps) }}".to_owned()),
            ),
            (
                "VELNOR_REPORT_DEFAULT_BRANCH".to_owned(),
                Value::Expr("${{ github.event.repository.default_branch }}".to_owned()),
            ),
        ]
        .into_iter()
        .chain(tests.then(|| {
            (
                "VELNOR_TESTS_RUN".to_owned(),
                Value::Expr("${{ steps.test.outputs.tests_run }}".to_owned()),
            )
        }))
        .collect(),
        ..RenderedStep::default()
    }
}

pub(crate) fn artifact(upload: bool) -> RenderedStep {
    RenderedStep {
        condition: upload.then(|| format!("steps.{REPORT_STEP}.outcome == 'success'")),
        continue_on_error: upload,
        name: if upload {
            "Upload cache metrics"
        } else {
            "Download cache metrics"
        }
        .to_owned(),
        uses: Some(
            if upload {
                UPLOAD_ARTIFACT
            } else {
                DOWNLOAD_ARTIFACT
            }
            .to_owned(),
        ),
        with: if upload {
            vec![
                (
                    "name".to_owned(),
                    Value::Expr("ci-metrics-${{ github.job }}".to_owned()),
                ),
                (
                    "path".to_owned(),
                    Value::Expr("${{ runner.temp }}/ci-metrics.json".to_owned()),
                ),
                (
                    "if-no-files-found".to_owned(),
                    Value::Text("error".to_owned()),
                ),
                ("retention-days".to_owned(), Value::Int(3)),
                ("overwrite".to_owned(), Value::Bool(true)),
            ]
        } else {
            vec![
                ("pattern".to_owned(), Value::Text("ci-metrics-*".to_owned())),
                (
                    "path".to_owned(),
                    Value::Expr("${{ runner.temp }}/ci-metrics".to_owned()),
                ),
            ]
        },
        ..RenderedStep::default()
    }
}
pub(crate) fn check(needs_max_bytes: usize, trusted_api: bool) -> RenderedStep {
    RenderedStep {
        name: "Check cache waste".to_owned(),
        run: Some("velnor-ci report --check \"$RUNNER_TEMP/ci-metrics\"".to_owned()),
        env: vec![(
            "VELNOR_REPORT_NEEDS".to_owned(),
            Value::BoundedExpr {
                text: "${{ toJSON(needs) }}".to_owned(),
                max_bytes: needs_max_bytes,
            },
        )].into_iter().chain(trusted_api.then(|| (
            "GITHUB_TOKEN".to_owned(),
            Value::Expr("${{ (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository) && github.token || '' }}".to_owned()),
        ))).chain([(
            "VELNOR_REPORT_DEFAULT_BRANCH".to_owned(),
            Value::Expr("${{ github.event.repository.default_branch }}".to_owned()),
        )]).collect(),
        ..RenderedStep::default()
    }
}
