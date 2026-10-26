// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
use std::process::Command;

const MINIMUM: (u32, u32, u32) = (1, 98, 1);

fn fail(message: &str) -> ! {
    eprintln!("velnor-actions-tofu MSRV gate: {message}");
    std::process::exit(1);
}

fn parse_release(line: &str) -> Option<(u32, u32, u32)> {
    let version = line.strip_prefix("release: ")?.split('-').next()?;
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn main() {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let output = Command::new(&rustc)
        .arg("-vV")
        .output()
        .unwrap_or_else(|err| {
            fail(&format!("cannot run {rustc} -vV: {err}"));
        });
    if !output.status.success() {
        fail(&format!("{rustc} -vV exited unsuccessfully"));
    }
    let text = String::from_utf8(output.stdout).unwrap_or_else(|_| {
        fail(&format!("{rustc} -vV emitted non-UTF-8 output"));
    });
    let release = text.lines().find_map(parse_release).unwrap_or_else(|| {
        fail(&format!("cannot parse release from {rustc} -vV"));
    });
    if release < MINIMUM {
        fail(&format!(
            "rustc {}.{}.{} below minimum {}.{}.{} for hcl-rs 0.19.8",
            release.0, release.1, release.2, MINIMUM.0, MINIMUM.1, MINIMUM.2
        ));
    }
}
