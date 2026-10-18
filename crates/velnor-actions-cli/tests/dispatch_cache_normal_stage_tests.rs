use super::*;

use std::collections::BTreeMap;
use std::fs;

fn inputs(key: &str, runner_temp: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("RUNNER_TEMP".to_owned(), runner_temp.to_owned()),
        (KEY_ENV.to_owned(), key.to_owned()),
    ])
}

fn read(values: &BTreeMap<String, String>) -> Result<String, &'static str> {
    read_stage_path(|name| values.get(name).cloned().ok_or(env::VarError::NotPresent))
}

fn key(identity: char) -> String {
    format!(
        "mise-tools-v2-payload-v3-{}",
        identity.to_string().repeat(64)
    )
}

#[test]
fn stage_path_uses_blake3_of_the_exact_key_and_excludes_run_coordinates() {
    let cache_key = key('a');
    let values = inputs(&cache_key, "/runner/temp");
    let first = read(&values).expect("valid key and runner temp");
    let second = read(&values).expect("same key and runner temp");
    let expected_digest = blake3::hash(cache_key.as_bytes()).to_hex();

    assert_eq!(
        first,
        format!("/runner/temp/velnor/normal-cache-v3/mise_tools/{expected_digest}/payload")
    );
    assert_eq!(first, second);
    assert!(!first.contains("run-"));
    assert_ne!(
        first,
        read(&inputs(&key('b'), "/runner/temp")).expect("different key")
    );
}

#[test]
fn stage_path_rejects_old_namespace_bad_identity_and_path_traversal() {
    for cache_key in [
        format!("mise-tools-v2-{}", "a".repeat(64)),
        format!("mise-tools-v2-payload-v2-{}", "a".repeat(64)),
        format!("mise-tools-v2-payload-v3-{}", "A".repeat(64)),
        "mise-tools-v2-payload-v3-short".to_owned(),
        format!("mise-tools-v2-payload-v3-{}\n", "a".repeat(64)),
    ] {
        assert_eq!(
            read(&inputs(&cache_key, "/runner/temp")),
            Err("normal_cache_key_invalid")
        );
    }
    for runner_temp in [
        "relative/temp",
        "/",
        "/runner/../other",
        "/runner//temp",
        "/runner/temp\nother",
    ] {
        assert_eq!(
            read(&inputs(&key('a'), runner_temp)),
            Err("normal_cache_runner_temp_invalid")
        );
    }
}

#[test]
fn stage_path_requires_the_runtime_key_input() {
    let values = BTreeMap::from([("RUNNER_TEMP".to_owned(), "/runner/temp".to_owned())]);
    assert_eq!(read(&values), Err("normal_cache_key_missing"));
}

#[test]
fn output_uses_the_owned_regular_output_file() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let output = temp.path().join("github-output");
    fs::write(&output, "prior=value\n").expect("create output file");
    let cache_key = key('a');
    let stage_path = read(&inputs(&cache_key, "/runner/temp")).expect("stage path");
    let body = format!("stage_path={stage_path}\n");

    super::super::append_owned_output(output.as_os_str(), body.as_bytes())
        .expect("append owned output");

    assert_eq!(
        fs::read_to_string(output).expect("output file text"),
        format!("prior=value\n{body}")
    );
}
