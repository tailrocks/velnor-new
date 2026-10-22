//! Runtime behavior of the exact embedded blob verifier.

use std::process::{Command, Output, Stdio};
use std::{io, io::Write};

use super::VERIFY_JS;

const VERSION_POLICY: &str = include_str!("../../../.velnor/version-policy.toml");

fn pinned_bun_version() -> &'static str {
    VERSION_POLICY
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == "bun").then(|| value.trim().trim_matches('"'))
        })
        .expect("Bun is pinned in version policy")
}

fn run(blob: &str, project: &str, shard: &str) -> io::Result<Output> {
    let mut child = Command::new("bun")
        .arg("-e")
        .arg(VERIFY_JS)
        .env("VELNOR_PW_PROJECT", project)
        .env("VELNOR_PW_SHARD", shard)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(blob.as_bytes())?;
    child.wait_with_output()
}

#[test]
fn embedded_verifier_handles_playwright_v2_project_closure_and_empty_shards() -> io::Result<()> {
    let pinned = pinned_bun_version();
    let version = match Command::new("bun").arg("--version").output() {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        _ => {
            eprintln!(
                "skip: Bun {pinned} is unavailable; embedded verifier runtime cases did not run"
            );
            return Ok(());
        }
    };
    if version != pinned {
        eprintln!(
            "skip: Bun {version} is installed, while version policy pins {pinned}; embedded verifier runtime cases did not run"
        );
        return Ok(());
    }

    let closure = include_str!("../tests/fixtures/playwright_blob_project_closure.jsonl");
    let no_deps = include_str!("../tests/fixtures/playwright_blob_no_deps.jsonl");
    let unfiltered = include_str!("../tests/fixtures/playwright_blob_unfiltered_projects.jsonl");
    let missing = include_str!("../tests/fixtures/playwright_blob_missing_project.jsonl");
    let empty = include_str!("../tests/fixtures/playwright_blob_empty_shard.jsonl");
    let unsupported_version = closure.replace("\"version\":2", "\"version\":1");
    let cases = [
        ("dependency and teardown closure", closure, "1/2", true),
        ("--no-deps metadata", no_deps, "1/2", true),
        ("dropped project filter", unfiltered, "1/2", false),
        ("missing selected project", missing, "1/2", false),
        ("empty shard with matching metadata", empty, "2/2", true),
        ("empty unsharded blob", empty, "", false),
        ("wrong shard metadata", closure, "2/2", false),
        (
            "unsupported blob version",
            &unsupported_version,
            "1/2",
            false,
        ),
    ];

    for (name, blob, shard, expected) in cases {
        let output = run(blob, "chromium", shard)?;
        assert_eq!(
            output.status.success(),
            expected,
            "{name}: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    Ok(())
}
