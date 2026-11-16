// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use crate::images::TARGETS;

pub const STANDIN_IMAGE: &str = "velnor-local/runner-server";
pub const CACHE_IMAGE: &str = "velnor-local/cache-server";
pub const RUNNER_IMAGE: &str = "velnor-local/runner";
pub const BUILD_FIX: &str = "velnor-local build";
pub const DOCKER_FIX: &str = "start OrbStack or Docker Desktop, then check `docker info`";

pub const PLAIN_IMAGES: [(&str, &str); 3] = [
    (STANDIN_IMAGE, "local/docker/runner-server"),
    (RUNNER_IMAGE, "local/docker/runner"),
    (CACHE_IMAGE, "-f local/docker/velnor-local/Dockerfile local"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub what: String,
    pub fix: Option<String>,
}

impl Part {
    #[must_use]
    pub fn ok(what: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            fix: None,
        }
    }

    #[must_use]
    pub fn miss(what: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            fix: Some(fix.into()),
        }
    }
}

#[must_use]
pub fn healthy(parts: &[Part]) -> bool {
    parts.iter().all(|part| part.fix.is_none())
}

#[must_use]
pub fn render(parts: &[Part]) -> String {
    parts.iter().fold(String::new(), |mut text, part| {
        let _ = match &part.fix {
            None => writeln!(text, "ok    {}", part.what),
            Some(fix) => writeln!(text, "miss  {}   fix: {fix}", part.what),
        };
        text
    })
}

#[must_use]
pub fn docker(info: Result<&str, ()>, socket: &str) -> Part {
    let Ok(info) = info else {
        return Part::miss("docker: not reachable", DOCKER_FIX);
    };
    let (os, architecture) = info.split_once('|').unwrap_or((info, ""));
    let name = if os.contains("OrbStack") {
        "OrbStack"
    } else {
        "Docker"
    };
    let platform = match architecture.trim() {
        "aarch64" | "arm64" => "linux/arm64".to_owned(),
        "x86_64" | "amd64" => "linux/amd64".to_owned(),
        other => format!("linux/{other}"),
    };
    let what = format!("docker: {name}, {platform}, {socket}");
    if platform == "linux/arm64" {
        Part::ok(what)
    } else {
        Part::miss(
            what,
            "use an Apple Silicon Mac: the runner images are linux/arm64",
        )
    }
}

#[must_use]
pub fn plain_images(missing: &[&str]) -> Part {
    let what = "stand-in, cache server and plain runner images".to_owned();
    let fixes: Vec<String> = PLAIN_IMAGES
        .iter()
        .filter(|(tag, _)| missing.contains(tag))
        .map(|(tag, context)| format!("docker build -t {tag} {context}"))
        .collect();
    if fixes.is_empty() {
        Part::ok(what)
    } else {
        Part::miss(
            format!("{what}: {} missing", missing.join(", ")),
            fixes.join(" && "),
        )
    }
}

#[must_use]
pub fn pair_images(missing: &[&str]) -> Part {
    let what = format!("pair images: {}", TARGETS.join(", "));
    if missing.is_empty() {
        Part::ok(what)
    } else {
        Part::miss(what, BUILD_FIX)
    }
}

#[must_use]
pub fn host(found: Result<&str, &str>) -> Part {
    match found {
        Ok(path) => Part::ok(format!("velnor-host with local mode: {path}")),
        Err(reason) => Part::miss(format!("velnor-host with local mode: {reason}"), BUILD_FIX),
    }
}
