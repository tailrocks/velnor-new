use std::error::Error;
use std::io::Write;
use std::process::{Command, Stdio};

use super::test_pins;
use super::{Family, family};

#[test]
fn generator_prepare_rerun_requires_source_target_and_exact_source_tag()
-> Result<(), Box<dyn Error>> {
    let generator = family::prepare_script(Family::Generator, &test_pins())?;
    let expected = extract_prepare_expected_assets(&generator)?;
    let predicate = extract_prepare_metadata_predicate(&generator)?;
    let tag_function = extract_prepare_tag_function(&generator)?;
    let canonical = prepare_release_json(expected, "0123456789abcdef0123456789abcdef01234567");
    let old_release = prepare_release_json(expected, "main");

    assert!(run_prepare_metadata_predicate(
        predicate, expected, &canonical
    )?);
    assert!(
        !run_prepare_metadata_predicate(predicate, expected, &old_release)?,
        "existing v0.1.4 with target_commitish=main passed prepare revalidation"
    );
    assert_prepare_tag_target(tag_function)?;
    Ok(())
}

fn extract_prepare_expected_assets(script: &str) -> Result<&str, Box<dyn Error>> {
    let start = script
        .find("readonly prepare_expected_assets='")
        .ok_or("generator prepare asset list is missing")?
        + "readonly prepare_expected_assets='".len();
    let end = script[start..]
        .find('\'')
        .map(|offset| start + offset)
        .ok_or("generator prepare asset list is unterminated")?;
    Ok(&script[start..end])
}

fn extract_prepare_metadata_predicate(script: &str) -> Result<&str, Box<dyn Error>> {
    let start = script
        .find("'.[0] as $release")
        .ok_or("generator prepare metadata predicate is missing")?
        + 1;
    let end = script[start..]
        .find(")' \\\n  <<<\"$matches\"")
        .map(|offset| start + offset + 1)
        .ok_or("generator prepare metadata predicate is unterminated")?;
    Ok(&script[start..end])
}

fn extract_prepare_tag_function(script: &str) -> Result<&str, Box<dyn Error>> {
    let start = script
        .find("assert_tag_target() {")
        .ok_or("generator prepare tag check is missing")?;
    let end = script[start..]
        .find("\n}\n")
        .map(|offset| start + offset + 2)
        .ok_or("generator prepare tag check is unterminated")?;
    Ok(&script[start..end])
}

fn prepare_release_json(expected: &str, target_commitish: &str) -> String {
    let assets = expected
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|name| {
            format!(
                r#"{{"name":{name},"state":"uploaded","size":1,"digest":"sha256:{}"}}"#,
                "a".repeat(64)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        r#"[{{"id":406452151,"tag_name":"v0.1.4","target_commitish":"{target_commitish}","url":"https://api.github.com/repos/tailrocks/velnor-new/releases/406452151","html_url":"https://github.com/tailrocks/velnor-new/releases/tag/v0.1.4","draft":false,"prerelease":false,"immutable":true,"assets":[{assets}]}}]"#
    )
}

fn run_prepare_metadata_predicate(
    predicate: &str,
    expected: &str,
    release: &str,
) -> Result<bool, Box<dyn Error>> {
    let mut child = Command::new("jq")
        .args([
            "-e",
            "--arg",
            "tag",
            "v0.1.4",
            "--arg",
            "sha",
            "0123456789abcdef0123456789abcdef01234567",
            "--argjson",
            "expected",
            expected,
            predicate,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("could not write generator release fixture")?
        .write_all(release.as_bytes())?;
    Ok(child.wait_with_output()?.status.success())
}

fn assert_prepare_tag_target(function: &str) -> Result<(), Box<dyn Error>> {
    let script = format!(
        "repository='tailrocks/velnor-new'\nsource_sha='0123456789abcdef0123456789abcdef01234567'\nprepare_tag='v0.1.4'\ngh() {{ test \"$*\" = 'api repos/tailrocks/velnor-new/git/ref/tags/v0.1.4' || return 1; printf '%s\\n' \"$GH_TAG_FIXTURE\"; }}\n{function}\nassert_tag_target\n"
    );
    let output = Command::new("bash")
        .args(["-euo", "pipefail", "-c", &script])
        .env(
            "GH_TAG_FIXTURE",
            r#"{"object":{"type":"commit","sha":"0123456789abcdef0123456789abcdef01234567"}}"#,
        )
        .output()?;
    assert!(
        output.status.success(),
        "exact generator source tag failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
