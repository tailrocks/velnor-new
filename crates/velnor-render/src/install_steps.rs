// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::HelperRelease;

use crate::mise_digest::pinned_sha256;
use crate::rules::{RenderedStep, SETUP_VELNOR_ACTION, Value};
use crate::steps::{MISE, MISE_VERSION};

pub(crate) fn install_tools(only: Option<&str>) -> RenderedStep {
    let mut with = vec![
        ("version".to_owned(), Value::Text(MISE_VERSION.to_owned())),
        ("sha256".to_owned(), pinned_sha256()),
        ("cache".to_owned(), Value::Bool(false)),
    ];
    if let Some(tool) = only {
        with.push(("install_args".to_owned(), Value::Text(tool.to_owned())));
    }
    RenderedStep {
        name: "Install tools".to_owned(),
        uses: Some(MISE.to_owned()),
        with,
        ..RenderedStep::default()
    }
}

pub(crate) fn install_mise() -> RenderedStep {
    RenderedStep {
        name: "Install mise".to_owned(),
        uses: Some(MISE.to_owned()),
        with: vec![
            ("version".to_owned(), Value::Text(MISE_VERSION.to_owned())),
            ("sha256".to_owned(), pinned_sha256()),
            ("install".to_owned(), Value::Bool(false)),
            ("cache".to_owned(), Value::Bool(false)),
        ],
        ..RenderedStep::default()
    }
}

pub(crate) fn install_helper(release: &HelperRelease) -> RenderedStep {
    let version = release.version();
    let mut with = vec![("version".to_owned(), Value::Text(version.to_string()))];
    with.extend(release.digests().iter().map(|(platform, digest)| {
        (
            format!("sha256-{}", platform.file_suffix()),
            Value::Text(digest.to_string()),
        )
    }));
    RenderedStep {
        name: format!("Install velnor-ci {version}"),
        uses: Some(format!("{SETUP_VELNOR_ACTION}@{}", release.action())),
        uses_comment: Some(version.release_tag()),
        with,
        ..RenderedStep::default()
    }
}
