//! MSRV compile-gate for the pinned `hcl-rs` parser.
//!
//! `hcl-rs` 0.19.8 declares no `rust-version` (Q1 pre-qualification), so
//! metadata cannot prove it compiles on the workspace MSRV. This gate
//! fails the build unless the invoking toolchain is at least 1.98.1,
//! the version T10 qualifies the parser against. It uses only `std`
//! and the `RUSTC` environment so the gate itself stays trivially
//! MSRV-safe.

use std::process::Command;

/// Minimum `rustc` release that may compile the pinned parser.
const MINIMUM: (u32, u32, u32) = (1, 98, 1);

/// Fail the build with a single-line message.
fn fail(message: &str) -> ! {
    eprintln!("velnor-actions-tofu MSRV gate: {message}");
    std::process::exit(1);
}

/// Parse `major.minor.patch` from a `rustc -vV` release line.
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
