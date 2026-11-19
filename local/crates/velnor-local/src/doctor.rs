// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

use crate::images::TARGETS;
use crate::pairs::Limits;

pub const STANDIN_IMAGE: &str = "velnor-local/runner-server";
pub const CACHE_IMAGE: &str = "velnor-local/cache-server";
pub const RUNNER_IMAGE: &str = "velnor-local/runner";
pub const BUILD_FIX: &str = "velnor-local build";
pub const DOCKER_FIX: &str = "start OrbStack or Docker Desktop, then check `docker info`";
pub const LOCAL_DOCKER_FIX: &str = "velnor-local needs a Docker daemon on this machine; to use a remote daemon or dind, run velnor-local and velnor-host inside it (Linux)";

pub const PLAIN_IMAGES: [(&str, &str); 3] = [
    (STANDIN_IMAGE, "local/docker/runner-server"),
    (RUNNER_IMAGE, "local/docker/runner"),
    (CACHE_IMAGE, "-f local/docker/velnor-local/Dockerfile local"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    pub what: String,
    pub fix: Option<String>,
    pub known: bool,
}

impl Part {
    #[must_use]
    pub fn ok(what: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            fix: None,
            known: true,
        }
    }

    #[must_use]
    pub fn unknown(what: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            fix: None,
            known: false,
        }
    }

    #[must_use]
    pub fn miss(what: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            what: what.into(),
            fix: Some(fix.into()),
            known: true,
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
            None if !part.known => writeln!(text, "n/a   {}", part.what),
            None => writeln!(text, "ok    {}", part.what),
            Some(fix) => writeln!(text, "miss  {}   fix: {fix}", part.what),
        };
        text
    })
}

#[must_use]
pub fn is_local(endpoint: &str) -> bool {
    endpoint.starts_with("unix://")
}

#[must_use]
pub fn docker(info: Result<&str, ()>, socket: Result<&str, ()>) -> Part {
    if let Ok(socket) = socket
        && !is_local(socket)
    {
        return Part::miss(
            format!("docker: {socket} is not a local socket"),
            LOCAL_DOCKER_FIX,
        );
    }
    let Ok(info) = info else {
        return Part::miss("docker: not reachable", DOCKER_FIX);
    };
    let Ok(socket) = socket else {
        return Part::miss("docker: context not readable", DOCKER_FIX);
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
pub fn plain_images(missing: Option<&[&str]>) -> Part {
    let what = "stand-in, cache server and plain runner images".to_owned();
    let Some(missing) = missing else {
        return Part::unknown(format!("{what}: unknown (Docker unreachable)"));
    };
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
pub fn pair_images(missing: Option<&[&str]>) -> Part {
    let what = format!("pair images: {}", TARGETS.join(", "));
    let Some(missing) = missing else {
        return Part::unknown(format!("{what}: unknown (Docker unreachable)"));
    };
    if missing.is_empty() {
        Part::ok(what)
    } else {
        Part::miss(what, BUILD_FIX)
    }
}

#[must_use]
pub fn host(found: Result<&str, &str>, fix: &str) -> Part {
    match found {
        Ok(path) => Part::ok(format!("velnor-host with local mode: {path}")),
        Err(reason) => Part::miss(format!("velnor-host with local mode: {reason}"), fix),
    }
}

const HEADROOM_BYTES: u64 = 1 << 30;

fn gib(bytes: u64) -> String {
    let tenths = bytes * 10 / (1 << 30);
    if tenths.is_multiple_of(10) {
        format!("{} GiB", tenths / 10)
    } else {
        format!("{}.{} GiB", tenths / 10, tenths % 10)
    }
}

/// Checks that the Docker VM can hold one pair (runner plus dind) and 1 GiB for the VM itself.
/// `total` is the VM memory that `docker info` reports, or `None` when it could not be read.
#[must_use]
pub fn vm_memory(total: Option<u64>, runner_memory: u64) -> Part {
    let limits = Limits {
        runner_memory_bytes: runner_memory,
        ..Limits::default()
    };
    let needed = limits.pair_memory_bytes() + HEADROOM_BYTES;
    let Some(total) = total else {
        return Part::unknown("docker vm memory: unknown (not reported by docker info)");
    };
    let what = format!(
        "docker vm memory: {} of {} needed (one pair: runner {} + dind {} + {} headroom)",
        gib(total),
        gib(needed),
        gib(runner_memory),
        gib(limits.pair_memory_bytes() - runner_memory),
        gib(HEADROOM_BYTES),
    );
    if total >= needed {
        Part::ok(what)
    } else {
        Part::miss(
            what,
            format!(
                "raise the VM memory to at least {}: OrbStack Settings > System > Memory limit (Docker Desktop: Settings > Resources), or lower --runner-memory",
                gib(needed)
            ),
        )
    }
}
