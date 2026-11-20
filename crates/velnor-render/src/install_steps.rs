// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, Platform};

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

const RELEASES: &str = "https://github.com/tailrocks/velnor-new/releases/download";

const INSTALL: &str = "set -euo pipefail; fail() { printf '%s\\n' \"::error title=velnor-gen::$1\"; exit 1; }; [ -n \"$VELNOR_GEN_ARCHIVE\" ] && [ -n \"$VELNOR_GEN_SHA256\" ] || fail \"no pinned velnor-gen archive for $RUNNER_OS $RUNNER_ARCH\"; directory=\"$RUNNER_TEMP/velnor-gen-trusted-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT\"; rm -rf \"$directory\"; mkdir -m 700 \"$directory\"; archive=\"$directory/$VELNOR_GEN_ARCHIVE\"; curl --fail --silent --show-error --location --retry 3 --max-filesize 268435456 --output \"$archive\" \"$VELNOR_GEN_RELEASE/$VELNOR_GEN_ARCHIVE\" || fail \"downloading $VELNOR_GEN_ARCHIVE failed\"; if command -v sha256sum > /dev/null; then actual=$(sha256sum \"$archive\" | cut -d ' ' -f 1); else actual=$(shasum -a 256 \"$archive\" | cut -d ' ' -f 1); fi; [ \"$actual\" = \"$VELNOR_GEN_SHA256\" ] || fail \"SHA-256 mismatch for $VELNOR_GEN_ARCHIVE: expected $VELNOR_GEN_SHA256, downloaded $actual. velnor-gen was not installed\"; tar --extract --gzip --no-same-owner --no-recursion --file \"$archive\" --directory \"$directory\" velnor-gen || fail \"extracting velnor-gen from $VELNOR_GEN_ARCHIVE failed\"; [ -f \"$directory/velnor-gen\" ] && [ ! -L \"$directory/velnor-gen\" ] || fail \"$VELNOR_GEN_ARCHIVE does not hold velnor-gen as a regular file\"; chmod 500 \"$directory/velnor-gen\"; printf '%s\\n' \"$directory\" >> \"$GITHUB_PATH\"";

fn choice(value: impl Fn(Platform) -> Option<String>) -> String {
    let choices: Vec<String> = Platform::ALL
        .into_iter()
        .filter_map(|platform| {
            let text = value(platform)?;
            let (os, arch) = platform.runner_key().split_once('-').unwrap_or(("", ""));
            Some(format!(
                "runner.os == '{os}' && runner.arch == '{arch}' && '{text}'"
            ))
        })
        .collect();
    format!("${{{{ {} || '' }}}}", choices.join(" || "))
}

pub(crate) fn install_generator_step(release: &HelperRelease) -> RenderedStep {
    let digests = release.generator_digests();
    let digest = |platform: Platform| {
        digests
            .and_then(|found| found.get(&platform))
            .map(ToString::to_string)
    };
    let archive = |platform: Platform| {
        digests
            .is_some_and(|found| found.contains_key(&platform))
            .then(|| format!("velnor-gen-{}.tar.gz", platform.release_triple()))
    };
    RenderedStep {
        name: format!("Install velnor-gen {}", release.version()),
        run: Some("eval \"$VELNOR_GEN_INSTALL\"".to_owned()),
        env: vec![
            (
                "VELNOR_GEN_INSTALL".to_owned(),
                Value::Text(INSTALL.to_owned()),
            ),
            (
                "VELNOR_GEN_RELEASE".to_owned(),
                Value::Text(format!("{RELEASES}/{}", release.version().release_tag())),
            ),
            (
                "VELNOR_GEN_ARCHIVE".to_owned(),
                Value::Expr(choice(archive)),
            ),
            ("VELNOR_GEN_SHA256".to_owned(), Value::Expr(choice(digest))),
        ],
        ..RenderedStep::default()
    }
}
